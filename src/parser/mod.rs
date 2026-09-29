//! Parse sectioned or flow-style pipelines and separate source inventories.

mod declarations;
mod flow;
mod inventory;
mod keyword;
mod lexical;
mod operation;
mod sectioned;
mod source_map;

use std::fmt;

use crate::model::{
    CommandDef, CoverageRule, DirectoryDiscovery, Invocation, OperationDef, ProductDef,
};
use crate::paths::PathTemplate;
use crate::span::{address_of, columns_at, content_columns, Focus, Located, Place};
use crate::types::TypeExpr;

use self::flow::parse_flow;
use self::sectioned::{is_sectioned_document, parse_sectioned};

pub(crate) use self::declarations::{parse_use, UseSpec};
pub use self::inventory::{parse_source_inventory, render_source_inventory};
pub(crate) use self::inventory::{source_record_lines, split_document};
pub(crate) use self::keyword::{Header, Keyword};
pub(crate) use self::lexical::{glued_comment, strip_comment};
pub(crate) use self::source_map::{Rule, SourceMap, Step};

const SHELL_SOURCE_REMOVED: &str =
    "shell-source is no longer supported; make the command executable available on PATH";

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

    /// Mark `token`, a slice of the line being parsed, as what the error is about.
    pub(crate) fn at_token(mut self, token: &str) -> Self {
        self.location
            .focus
            .get_or_insert(Focus::Address(address_of(token)));
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
            let token = match self.location.focus.take() {
                Some(Focus::Address(address)) => columns_at(line, &address),
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
    /// Every statement before `error`, or in the whole text without one.
    pub(crate) statements: Vec<Statement>,
    /// The first line that does not parse. Parsing stops there, and lowering
    /// reports it only when no statement before it fails, so errors are
    /// found in line order.
    pub(crate) error: Option<ParseError>,
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
    /// A `stage name:` header, by the stage's full name.
    Stage {
        name: String,
        place: Place,
    },
    /// A `source` declaration or an entry of a `products:` section.
    Product(ProductDef, Place),
    Discover(DirectoryDiscovery),
    Operation(OperationDef, Place),
    Constraint(CoverageRule, Rule),
    Command(CommandDef, Place),
    Path(PathRule),
    /// A step whose outputs are declared elsewhere, from a `pipeline:` section.
    Step(Invocation, Step),
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
    pub(crate) outputs: Vec<FlowOutput>,
    pub(crate) step: Step,
}

#[derive(Clone, Debug)]
pub(crate) struct FlowOutput {
    pub(crate) name: String,
    pub(crate) artifact_type: Option<TypeExpr>,
    pub(crate) dimensions: Option<Vec<String>>,
}

/// What a document may hold. A pipeline holds neither input rules nor
/// records; a `.spitin` recipe holds both, beside source paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Pipeline,
    Recipe,
}

/// Parse a pipeline's statements, in the sectioned or the flow form.
pub(crate) fn parse_syntax(text: &str) -> Syntax {
    if is_sectioned_document(text) {
        parse_sectioned(text)
    } else {
        parse_flow(text)
    }
}

impl Syntax {
    fn push(&mut self, original: &str, number: usize, kind: StatementKind) {
        self.statements.push(Statement {
            place: Place::new(number, content_columns(original)),
            kind,
        });
    }
}
