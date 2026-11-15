//! The errors the parser reports: a [`ParseErrorKind`] at a line. Each
//! area of the parser has its own enum, in a file of its own, and
//! [`fmt::Display`] words each variant.

use std::fmt;

use crate::span::{address_of, columns_at, content_columns, Focus, Located, Place};
use crate::template::{CommandProblem, PathProblem};
use crate::types::TypeParseError;

mod lexical;
pub use lexical::LexicalError;
mod check;
pub use check::CheckError;
mod declarations;
pub use declarations::DeclarationError;
mod operation;
pub use operation::OperationError;
mod flow;
pub use flow::FlowError;
mod rules;
pub use rules::{RuleError, RuleForm};
mod inventory;
pub use inventory::InventoryError;
mod review;
pub use review::ReviewError;
mod command;
pub use command::CommandError;
mod entities;
pub use entities::EntityError;
mod resources;
pub use resources::ResourceError;
mod body;
pub use body::BodyError;
mod lower;
pub use lower::LowerError;
mod order;
pub use order::OrderError;
mod recipe;
pub use recipe::RecipeError;
mod tools;
pub use tools::ToolError;

/// A parse error, with the line it is on.
pub type ParseError = Located<ParseFailure>;

/// What went wrong while parsing a line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseFailure {
    /// Boxed, so that a `Result` holding a parse error stays small on the
    /// path that succeeds.
    pub kind: Box<ParseErrorKind>,
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

/// What a parse error is. [`fmt::Display`] words it. The enums of each area
/// are not part of the library's API: match the variants here, or read the
/// wording.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// An error of a stage after parsing, reported at a line of the text, with the wording the stage gave it.
    Other { message: String },
    /// A flow step calls an operation that has not been declared yet.
    UndeclaredOperation { name: String },
    /// Records written in a pipeline; they belong in a `.spitout`. Each
    /// line they take, so that one error covers them all.
    MisplacedRecords { lines: Vec<usize> },
    /// An operation's header opens a body, but no step follows it.
    EmptyBody { name: String },
    /// A clause, `@keyword(...)`, that may not be written where it is.
    ExpectedClause { site: ClauseSite },
    /// A clause, `@keyword(...)`, written twice.
    DuplicateClause { keyword: String },
    /// An error of names, extensions and comma-separated lists.
    Lexical(LexicalError),
    /// An error of `check` declarations and the check lists of `@check(...)` clauses.
    Check(CheckError),
    /// An error of `source`, `use`, `path`, `dimensions`, `type` and `exclude` lines and a call's selectors.
    Declaration(DeclarationError),
    /// An error of `operation` declarations: ports, outputs, clauses and files written beside another.
    Operation(OperationError),
    /// An error of statements: keywords, stages, defaults and steps.
    Flow(FlowError),
    /// An error of a recipe's `require` and `exclude` rules.
    Rule(RuleError),
    /// An error of a `.spitout`'s sections and records.
    Inventory(InventoryError),
    /// An error of `review` declarations and review lists.
    Review(ReviewError),
    /// An error of `command` and `verify` lines and the shell operators they exclude.
    Command(CommandError),
    /// An error of `default entities:` spellings.
    Entity(EntityError),
    /// An error of `resources` lines.
    Resource(ResourceError),
    /// An error of the steps of an operation's body.
    Body(BodyError),
    /// A path template that does not parse, as the template parser words it.
    PathTemplate(PathProblem),
    /// The command template of `owner`, such as ``check `ndim` ``, does not parse.
    CommandOf {
        owner: String,
        problem: CommandProblem,
    },
    /// A type that does not parse, as the type parser words it.
    Type(TypeParseError),
    /// An error of lowering a document: duplicates, imports, calls and bodies.
    Lower(LowerError),
    /// An error of the dimension order of products.
    Order(OrderError),
    /// An error of reading a `.spitin`: its header and `inputs:` block.
    Recipe(RecipeError),
    /// An error of `tool`, `uses` and `anchored` lines and the lists of tools.
    Tool(ToolError),
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other { message } => f.write_str(message),
            Self::UndeclaredOperation { name } => {
                write!(f, "operation `{name}` must be declared before its first flow step")
            }
            Self::MisplacedRecords { .. } => f.write_str(
                "`sources:` and `contexts:` records belong in a .spitout, not a pipeline",
            ),
            Self::EmptyBody { name } => write!(
                f,
                "operation `{name}` ends its header with `:` but has no steps; indent each beneath it as `output = operation(inputs)`, or drop the `:` and give it a `command`"
            ),
            Self::ExpectedClause { site } => f.write_str(site.expected()),
            Self::DuplicateClause { keyword } => write!(f, "duplicate `@{keyword}(...)`"),
            Self::Lexical(error) => error.fmt(f),
            Self::Check(error) => error.fmt(f),
            Self::Declaration(error) => error.fmt(f),
            Self::Operation(error) => error.fmt(f),
            Self::Flow(error) => error.fmt(f),
            Self::Rule(error) => error.fmt(f),
            Self::Inventory(error) => error.fmt(f),
            Self::Review(error) => error.fmt(f),
            Self::Command(error) => error.fmt(f),
            Self::Entity(error) => error.fmt(f),
            Self::Resource(error) => error.fmt(f),
            Self::Body(error) => error.fmt(f),
            Self::PathTemplate(problem) => problem.fmt(f),
            Self::CommandOf { owner, problem } => write!(f, "{owner}: {problem}"),
            Self::Type(error) => error.fmt(f),
            Self::Lower(error) => error.fmt(f),
            Self::Order(error) => error.fmt(f),
            Self::Recipe(error) => error.fmt(f),
            Self::Tool(error) => error.fmt(f),
        }
    }
}

impl From<LexicalError> for ParseErrorKind {
    fn from(error: LexicalError) -> Self {
        Self::Lexical(error)
    }
}

impl From<CheckError> for ParseErrorKind {
    fn from(error: CheckError) -> Self {
        Self::Check(error)
    }
}

impl From<DeclarationError> for ParseErrorKind {
    fn from(error: DeclarationError) -> Self {
        Self::Declaration(error)
    }
}

impl From<OperationError> for ParseErrorKind {
    fn from(error: OperationError) -> Self {
        Self::Operation(error)
    }
}

impl From<FlowError> for ParseErrorKind {
    fn from(error: FlowError) -> Self {
        Self::Flow(error)
    }
}

impl From<RuleError> for ParseErrorKind {
    fn from(error: RuleError) -> Self {
        Self::Rule(error)
    }
}

impl From<InventoryError> for ParseErrorKind {
    fn from(error: InventoryError) -> Self {
        Self::Inventory(error)
    }
}

impl From<ReviewError> for ParseErrorKind {
    fn from(error: ReviewError) -> Self {
        Self::Review(error)
    }
}

impl From<CommandError> for ParseErrorKind {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl From<EntityError> for ParseErrorKind {
    fn from(error: EntityError) -> Self {
        Self::Entity(error)
    }
}

impl From<ResourceError> for ParseErrorKind {
    fn from(error: ResourceError) -> Self {
        Self::Resource(error)
    }
}

impl From<BodyError> for ParseErrorKind {
    fn from(error: BodyError) -> Self {
        Self::Body(error)
    }
}

impl From<LowerError> for ParseErrorKind {
    fn from(error: LowerError) -> Self {
        Self::Lower(error)
    }
}

impl From<OrderError> for ParseErrorKind {
    fn from(error: OrderError) -> Self {
        Self::Order(error)
    }
}

impl From<RecipeError> for ParseErrorKind {
    fn from(error: RecipeError) -> Self {
        Self::Recipe(error)
    }
}

impl From<ToolError> for ParseErrorKind {
    fn from(error: ToolError) -> Self {
        Self::Tool(error)
    }
}

/// Where an `@keyword(...)` clause is written, which says what may be.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClauseSite {
    /// After a `source` declaration.
    AfterSource,
    /// On an input port.
    OnPort,
    /// After an operation's outputs, where `@min` and `@drop` are read only to say what to write.
    AfterSignature,
    /// After one of an operation's outputs.
    AfterOutput,
    /// On an input of a call.
    OnInput,
}

impl ClauseSite {
    /// What may be written there.
    pub(super) fn expected(self) -> &'static str {
        match self {
            Self::AfterSource => "expected `@check(...)` or `@review(...)` after a source",
            Self::OnPort => {
                "expected `@min(count)` on a many input, or `@check(...)` or `@review(...)`"
            }
            Self::AfterSignature => {
                "nothing follows an operation's outputs; write `@min(count)` on its many input"
            }
            Self::AfterOutput => {
                "nothing follows an operation's outputs but their `@check(...)` and `@review(...)`; \
                 write `@min(count)` on its many input"
            }
            Self::OnInput => {
                "expected `@vary(dimension)`, `@where(dimension=value, ...)`, `@same(dimension, ...)`, or `@each(dimension, ...)`"
            }
        }
    }
}

impl ParseError {
    /// An error on `line` of the given kind.
    pub(crate) fn of(line: usize, kind: impl Into<ParseErrorKind>) -> Self {
        let mut error = Located::unplaced(ParseFailure {
            kind: Box::new(kind.into()),
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
            if let Some(columns) = match &self.location.focus {
                Some(Focus::Address(address)) if line.contains('\n') => columns_at(line, address),
                _ => None,
            } {
                // A statement of several lines: the token's line is the first
                // line, plus the lines before the token.
                let prefix = &line[..columns.start];
                let offset = prefix.rfind('\n').map_or(0, |index| index + 1);
                let end = line[columns.clone()]
                    .find('\n')
                    .map_or(columns.end, |index| columns.start + index);
                self.location.line =
                    Some(self.line() + prefix.bytes().filter(|&byte| byte == b'\n').count());
                self.location.columns = Some(columns.start - offset..end - offset);
                self.location.focus = None;
                return self;
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
