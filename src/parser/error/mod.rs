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
// AREA-MODS

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

/// What a parse error is. [`fmt::Display`] words it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// An error with no variant of its own, with its wording.
    Syntax { message: String },
    /// A flow step calls an operation that has not been declared yet.
    UndeclaredOperation { name: String },
    /// Records written in a pipeline; they belong in a `.spitout`. Each
    /// line they take, so that one error covers them all.
    MisplacedRecords { lines: Vec<usize> },
    /// An operation's header opens a body, but no step follows it.
    EmptyBody { name: String },
    /// A clause, `@keyword(...)`, that may not be written where it is.
    ExpectedClause { site: ClauseSite },
    /// An error of `lexical`.
    Lexical(LexicalError),
    /// An error of `check`.
    Check(CheckError),
    /// An error of `declarations`.
    Declaration(DeclarationError),
    /// An error of `operation`.
    Operation(OperationError),
    /// An error of `flow`.
    Flow(FlowError),
    /// An error of `rules`.
    Rule(RuleError),
    /// An error of `inventory`.
    Inventory(InventoryError),
    /// An error of `review`.
    Review(ReviewError),
    /// An error of `command`.
    Command(CommandError),
    /// An error of `entities`.
    Entity(EntityError),
    /// An error of `resources`.
    Resource(ResourceError),
    /// An error of `body`.
    Body(BodyError),
    /// A path template that does not parse, as the template parser words it.
    PathTemplate(PathProblem),
    /// A command template that does not parse, as the template parser words it.
    CommandTemplate(CommandProblem),
    /// The command template of `owner`, such as ``check `ndim` ``, does not parse.
    CommandOf {
        owner: String,
        problem: CommandProblem,
    },
    /// A type that does not parse, as the type parser words it.
    Type(TypeParseError),
    // AREA-KINDS
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { message } => f.write_str(message),
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
            Self::CommandTemplate(problem) => problem.fmt(f),
            Self::CommandOf { owner, problem } => write!(f, "{owner}: {problem}"),
            Self::Type(error) => error.fmt(f),
            // AREA-DISPLAY
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

// AREA-FROMS

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
    /// An error on `line` that has no variant of its own yet, with its wording.
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        Self::of(
            line,
            ParseErrorKind::Syntax {
                message: message.into(),
            },
        )
    }

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
