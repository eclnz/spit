//! Parse flow-style pipelines and separate source inventories.

mod body;
mod check;
mod command;
mod declarations;
mod entities;
pub(crate) mod error;
mod flow;
mod inventory;
mod keyword;
mod layout;
mod lexical;
mod operation;
mod render_inventory;
mod resources;
mod review;
mod rules;
mod scan;
mod source_map;
mod tools;

pub(crate) use entities::EntitiesDeclaration;
pub use error::{ClauseSite, ParseError, ParseErrorKind};

use std::ops::Range;

use crate::model::{
    CheckDef, CheckKind, CommandDef, CommandRole, CoverageRule, DefaultChecks, DefaultReviews,
    Invocation, OperationDef, ProductDef, ResourcesDef, ReviewDef, StepOutput, ToolList,
    TypeChecking, UsesDef,
};
use crate::span::{content_columns, Place};
use crate::template::PathTemplate;

use self::flow::parse_flow;

pub(crate) use self::body::empty_body;
pub(crate) use self::declarations::{parse_use, UseSpec};
pub use self::inventory::parse_source_inventory;
pub(crate) use self::inventory::{
    document_records, parse_inventory_recovering, source_record_lines,
};
pub use self::keyword::keywords_json;
pub(crate) use self::keyword::{File, Header, Keyword};
pub(crate) use self::layout::{Block, Head, Layout, Line};
pub(crate) use self::lexical::without_bom;
pub(crate) use self::render_inventory::as_read_back;
pub use self::render_inventory::render_source_inventory;
pub(crate) use self::source_map::{CommandPlaces, Rule, SourceMap, Step};

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
    /// The pipeline's optional output root.
    OutputRoot(std::path::PathBuf),
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
    Exclude(declarations::ExcludeRule, Option<String>, Place),
    /// A `prefer path` rule, and its reason from the line's comment.
    Prefer(crate::template::PathGlob, Option<String>),
    /// A command, with where it and each of its lines are.
    Command(CommandDef, CommandPlaces),
    /// A `resources` line, and where its operation's name is.
    Resources(ResourcesDef, Place),
    /// A `tool` line's names, and where the line's text is.
    Tool(Vec<String>, Place),
    /// A `uses` line, and where its operation's name is.
    Uses(UsesDef, Place),
    /// An `anchored` line's operations, and where the line's text is.
    Anchored(Vec<String>, Place),
    /// A `default uses:` line: the tools of every operation declared in
    /// `stage` or, outside every stage, in the file.
    DefaultUses {
        stage: Option<String>,
        tools: ToolList,
        /// Where the list sits.
        place: Place,
    },
    /// A `check` declaration, and where its name is.
    Check(CheckDef, Place),
    /// A `review` declaration, and where its name is.
    Review(ReviewDef, Place),
    /// A recipe's `review name from file` line, and where the name is.
    ReviewFile {
        review: String,
        file: String,
        place: Place,
    },
    /// A `dimensions [...]` line: the pipeline's dimension order.
    Dimensions(Vec<String>),
    /// A `types` line: how much of the pipeline must be typed.
    Types(TypeChecking),
    /// A `type Name<Param, ...>` line: a parameterized type and the names
    /// of its parameters, in order.
    TypeDecl(String, Vec<String>),
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
        /// The extension the list is for, as in `default check .nii.gz:`.
        extension: Option<String>,
        checks: DefaultChecks,
        /// Where the list sits.
        place: Place,
    },
    /// A `review:` line: the reviews of every output in `stage` or, outside
    /// every stage, in the whole file.
    DefaultReviews {
        stage: Option<String>,
        reviews: DefaultReviews,
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
        let reason = crate::span::comment_text(original).map(str::to_owned);
        Ok(Self::Exclude(rule, reason, place))
    }

    /// A `prefer path` rule, `declaration` being the text after its keyword
    /// in `original`.
    fn prefer(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let path = declarations::parse_prefer(declaration, number)?;
        let reason = crate::span::comment_text(original).map(str::to_owned);
        Ok(Self::Prefer(path, reason))
    }

    /// A `check` declaration, `declaration` being the text after its
    /// keyword in `original`.
    fn check(
        original: &str,
        declaration: &str,
        number: usize,
        kind: CheckKind,
    ) -> Result<Self, ParseError> {
        let mut check = check::parse_check(declaration, number)?;
        check.kind = kind;
        let place = source_map::name_place(original, number, declaration, &check.name);
        Ok(Self::Check(check, place))
    }

    /// A `review` line, `declaration` being the text after its keyword in
    /// `original`.
    fn review(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        Ok(match review::parse_review(declaration, number)? {
            review::ReviewLine::Declare(review) => {
                let place = source_map::name_place(original, number, declaration, &review.name);
                Self::Review(review, place)
            }
            review::ReviewLine::File { review, file } => {
                let place = source_map::name_place(original, number, declaration, &review);
                Self::ReviewFile {
                    review,
                    file,
                    place,
                }
            }
        })
    }

    /// A command, or a `verify` command, for an operation.
    fn command(
        original: &str,
        declaration: &str,
        number: usize,
        role: CommandRole,
    ) -> Result<Self, ParseError> {
        let (command, places) = command::parse_command(original, declaration, number, role)?;
        Ok(Self::Command(command, places))
    }

    /// A `resources` line, `declaration` being the text after its keyword
    /// in `original`.
    fn resources(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let resources = resources::parse_resources(declaration, number)?;
        let place = source_map::name_place(original, number, declaration, &resources.operation);
        Ok(Self::Resources(resources, place))
    }

    /// A `tool` line, `declaration` being the text after its keyword in
    /// `original`.
    fn tool(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let names = tools::parse_tool_names(declaration, number)?;
        Ok(Self::Tool(
            names,
            source_map::tail_place(original, number, declaration),
        ))
    }

    /// An `anchored` line, `declaration` being the text after its keyword
    /// in `original`.
    fn anchored(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let names = tools::parse_anchored(declaration, number)?;
        Ok(Self::Anchored(
            names,
            source_map::tail_place(original, number, declaration),
        ))
    }

    /// A `uses` line, `declaration` being the text after its keyword in
    /// `original`.
    fn uses(original: &str, declaration: &str, number: usize) -> Result<Self, ParseError> {
        let uses = tools::parse_uses(declaration, number)?;
        let place = source_map::name_place(original, number, declaration, &uses.operation);
        Ok(Self::Uses(uses, place))
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
