//! Parse flow-style pipelines and separate source inventories.

mod body;
mod check;
mod command;
mod declarations;
mod entities;
mod flow;
mod inventory;
mod keyword;
mod layout;
mod lexical;
mod operation;
mod render_inventory;
mod rules;
mod source_map;

pub(crate) use entities::EntitiesDeclaration;

use std::fmt;
use std::ops::Range;

use crate::model::{
    CheckDef, CommandDef, CommandRole, CoverageRule, DefaultChecks, Invocation, OperationDef,
    ProductDef, StepOutput, TypeChecking,
};
use crate::paths::PathTemplate;
use crate::span::{address_of, columns_at, content_columns, Focus, Located, Place};

use self::flow::parse_flow;

pub(crate) use self::body::empty_body;
pub(crate) use self::declarations::{parse_use, ExcludeLine, UseSpec};
pub use self::inventory::parse_source_inventory;
pub(crate) use self::inventory::{
    document_records, parse_inventory_recovering, source_record_lines,
};
pub(crate) use self::keyword::{Header, Keyword};
pub(crate) use self::layout::{Block, Head, Layout, Line};
pub(crate) use self::lexical::{glued_comment, strip_comment, without_bom};
pub(crate) use self::render_inventory::as_read_back;
pub use self::render_inventory::render_source_inventory;
pub(crate) use self::source_map::{Rule, SourceMap, Step};

const SHELL_SOURCE_ERROR: &str = "`shell-source` starts no statement: a command runs a program, not a shell function, so make the program executable and put it on PATH";

/// A parse error, with the line it is on.
pub type ParseError = Located<ParseFailure>;

/// What went wrong while parsing a line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseFailure {
    pub kind: ParseErrorKind,
    pub message: String,
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Errors that callers may want to treat specially; everything else is `Syntax`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    Syntax,
    /// A flow step calls an operation that has not been declared yet.
    UndeclaredOperation {
        name: String,
    },
    /// Records written in a pipeline; they belong in a `.spitout`. Each
    /// line they take, so that one error covers them all.
    MisplacedRecords {
        lines: Vec<usize>,
    },
    /// An operation's header opens a body, but no step follows it.
    EmptyBody,
}

impl ParseError {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        let mut error = Located::unplaced(ParseFailure {
            kind: ParseErrorKind::Syntax,
            message: message.into(),
        });
        error.location.line = Some(line);
        error
    }

    /// The line the error is on; every parse error has one.
    pub fn line(&self) -> usize {
        self.location.line.unwrap_or_default()
    }

    /// Which kind of error it is, for callers that treat some specially.
    pub fn kind(&self) -> &ParseErrorKind {
        &self.error.kind
    }

    /// This error, as `kind`.
    pub(crate) fn with_kind(mut self, kind: ParseErrorKind) -> Self {
        self.error.kind = kind;
        self
    }

    /// Mark `token`, a slice of the line being parsed, as what the error is about.
    pub(crate) fn at_token(mut self, token: &str) -> Self {
        self.location
            .focus
            .get_or_insert_with(|| Focus::Address(address_of(token)));
        self
    }

    /// Point at `place` unless the error already points somewhere, for an
    /// error found after its line was parsed.
    pub(crate) fn within(mut self, place: &Place) -> Self {
        if self.location.columns.is_none() && self.location.focus.is_none() {
            self.location.columns = Some(place.columns.clone());
        }
        self
    }

    /// Resolve the marked token to columns of `line`, the text it was sliced
    /// from; without one, point at the line's content.
    pub(crate) fn locate(mut self, line: &str) -> Self {
        if self.location.columns.is_none() {
            if let Some(Focus::Address(address)) =
                self.location.focus.as_ref().filter(|_| line.contains('\n'))
            {
                let base = line.as_ptr() as usize;
                if let Some(start) = address
                    .start
                    .checked_sub(base)
                    .filter(|&start| start < line.len())
                {
                    let prefix = &line[..start];
                    let offset = prefix.rfind('\n').map_or(0, |index| index + 1);
                    let end = address.end.saturating_sub(base).min(line.len());
                    let end = line[start..end]
                        .find('\n')
                        .map_or(end, |index| start + index);
                    self.location.line =
                        Some(self.line() + prefix.bytes().filter(|&byte| byte == b'\n').count());
                    self.location.columns = Some(start - offset..end - offset);
                    self.location.focus = None;
                    return self;
                }
            }
            let token = match self.location.focus.take() {
                Some(Focus::Address(address)) => columns_at(line, &address),
                Some(other @ Focus::Imported(_)) => {
                    self.location.focus = Some(other);
                    None
                }
                _ => None,
            };
            self.location.columns = Some(token.unwrap_or_else(|| content_columns(line)));
        }
        self
    }
}

/// A pipeline as written: its statements in source order, before lowering
/// turns them into a [`Pipeline`]. Each keeps where its parts sit.
#[derive(Clone, Debug, Default)]
pub(crate) struct Syntax {
    /// Every statement read, up to where parsing stopped.
    pub(crate) statements: Vec<Statement>,
    /// Each statement that does not parse, with how many statements were
    /// read before it, so that lowering can report errors in the order of
    /// their lines. Keep in step with `lower` in `src/lower/mod.rs`, which
    /// orders them among the statements' own errors.
    pub(crate) errors: Vec<(usize, ParseError)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Statement {
    /// The statement's line content, for errors about it as a whole.
    pub(crate) place: Place,
    pub(crate) kind: StatementKind,
}

#[derive(Clone, Debug)]
pub(crate) enum StatementKind {
    /// A `use` line; the definitions it brings in are loaded separately.
    Import,
    /// The pipeline's optional dataset root.
    Root(std::path::PathBuf),
    /// A `stage name:` header, by the stage's full name.
    Stage {
        name: String,
        place: Place,
    },
    /// A `source` declaration.
    Product(ProductDef, Place),
    /// An `operation` declaration, where its name sits, and the stage its
    /// line is in, if any. The operation is global either way.
    /// A body that holds no step is an error where the body ends: on the
    /// line that ends it, or the header's when the text does, whose columns
    /// are the last field. Keep in step with `empty_body` in
    /// `src/parser/body.rs`.
    Operation(OperationDef, Place, Option<String>, Range<usize>),
    Constraint(CoverageRule, Rule),
    /// An `exclude` rule, its reason from the line's comment, and where
    /// what it names sits.
    Exclude(declarations::ExcludeLine, Option<String>, Place),
    Command(CommandDef, Place),
    /// A `check` declaration, and where its name is.
    Check(CheckDef, Place),
    /// A `dimensions [...]` line: the pipeline's dimension order.
    Dimensions(Vec<String>),
    /// A `types` line: how much of the pipeline must be typed.
    Types(TypeChecking),
    /// A `default entities` line: how `{@entities}` is spelled, or a
    /// dimension's label in it, for the products of `stage` or, outside
    /// every stage, of the whole pipeline.
    Entities {
        stage: Option<String>,
        declaration: EntitiesDeclaration,
    },
    Path(PathRule),
    /// An `ext:` line: the default extension of `stage`, or, outside every
    /// stage, of the whole pipeline.
    Extension {
        stage: Option<String>,
        extension: String,
    },
    /// A `check:` line: the checks of every output in `stage` or, outside
    /// every stage, in the whole file.
    DefaultChecks {
        stage: Option<String>,
        checks: DefaultChecks,
        /// Where the list sits.
        place: Place,
    },
    /// A flow step, which declares its output products.
    FlowStep(FlowStep),
}

/// A `path:` or `path product:` rule.
#[derive(Clone, Debug)]
pub(crate) struct PathRule {
    /// The product it is for; without one, the default for `stage` or,
    /// outside every stage, for the whole pipeline.
    pub(crate) product: Option<String>,
    pub(crate) stage: Option<String>,
    pub(crate) template: PathTemplate,
    /// Where the template sits.
    pub(crate) place: Place,
}

/// A step written `outputs = operation(inputs)`. Each output declares a
/// product, whose type and dimensions may be left for lowering to infer.
#[derive(Clone, Debug)]
pub(crate) struct FlowStep {
    pub(crate) invocation: Invocation,
    pub(crate) outputs: Vec<StepOutput>,
    pub(crate) step: Step,
}

/// What a document may hold. A pipeline holds neither input rules nor
/// records; a `.spitin` recipe holds both, beside source paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Pipeline,
    Recipe,
}

/// Parse a pipeline's statements, laid out in `layout`.
pub(crate) fn parse_syntax(layout: &Layout) -> Syntax {
    parse_flow(layout)
}

impl StatementKind {
    /// A product declared by `declaration`, a slice of the line `original`.
    fn product(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let product = declarations::parse_product(declaration, number)?;
        let place = source_map::name_place(original, number, declaration, &product.name);
        Ok(Self::Product(product, place))
    }

    /// An operation declared by `declaration`, a slice of `original`, on a
    /// line in `stage`.
    fn operation(
        original: &str,
        declaration: &str,
        number: usize,
        stage: Option<String>,
    ) -> Result<Self, ParseError> {
        // A header ending in `:` opens a body of steps; the `:` is no part
        // of the signature.
        let declaration = declaration.trim();
        let signature = declaration
            .strip_suffix(':')
            .unwrap_or(declaration)
            .trim_end();
        let operation = operation::parse_operation(signature, number)?;
        let place = source_map::name_place(original, number, signature, &operation.name);
        Ok(Self::Operation(
            operation,
            place.clone(),
            stage,
            place.columns,
        ))
    }

    /// A `require` or conditional `exclude` rule, `rest` being the text
    /// after its `keyword` in `original`.
    fn constraint(
        original: &str,
        keyword: Keyword,
        rest: &str,
        number: usize,
    ) -> Result<Self, ParseError> {
        let rule = rules::parse_coverage_rule(keyword, rest, number)?;
        let place = source_map::rule_place(original, number, &rule);
        Ok(Self::Constraint(rule, place))
    }

    /// An `exclude` rule, `declaration` being the text after its keyword
    /// in `original`.
    fn exclude(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let rule = declarations::parse_exclude(declaration, number)?;
        let place = source_map::tail_place(original, number, declaration.trim());
        let reason = lexical::comment_text(original).map(str::to_owned);
        Ok(Self::Exclude(rule, reason, place))
    }

    /// A `check` declaration, `declaration` being the text after its
    /// keyword in `original`.
    fn check(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let check = check::parse_check(declaration, number)?;
        let place = source_map::name_place(original, number, declaration, &check.name);
        Ok(Self::Check(check, place))
    }

    /// A command, or a `verify` command, for an operation.
    fn command(
        original: &str,
        declaration: &str,
        number: usize,
        role: CommandRole,
    ) -> Result<Self, ParseError> {
        let command = command::parse_command(declaration, number, role)?;
        let place = source_map::tail_place(original, number, command.template.as_str());
        Ok(Self::Command(command, place))
    }
}

impl Syntax {
    fn push(&mut self, original: &str, number: usize, kind: StatementKind) {
        self.statements.push(Statement {
            place: Place::new(number, content_columns(original)),
            kind,
        });
    }

    /// Record `error`, from a statement that did not parse.
    fn fail(&mut self, error: ParseError) {
        self.errors.push((self.statements.len(), error));
    }
}
