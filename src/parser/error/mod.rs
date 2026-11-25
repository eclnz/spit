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

pub type ParseError = Located<ParseFailure>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseFailure {
    pub kind: Box<ParseErrorKind>,
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    Other {
        message: String,
    },
    UndeclaredOperation {
        name: String,
    },
    MisplacedRecords {
        lines: Vec<usize>,
    },
    EmptyBody {
        name: String,
    },
    ExpectedClause {
        site: ClauseSite,
    },
    DuplicateClause {
        keyword: String,
    },
    Lexical(LexicalError),
    Check(CheckError),
    Declaration(DeclarationError),
    Operation(OperationError),
    Flow(FlowError),
    Rule(RuleError),
    Inventory(InventoryError),
    Review(ReviewError),
    Command(CommandError),
    Entity(EntityError),
    Resource(ResourceError),
    Body(BodyError),
    PathTemplate(PathProblem),
    CommandOf {
        owner: String,
        problem: CommandProblem,
    },
    Type(TypeParseError),
    Lower(LowerError),
    Order(OrderError),
    Recipe(RecipeError),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClauseSite {
    AfterSource,
    OnPort,
    AfterSignature,
    AfterOutput,
    OnInput,
}

impl ClauseSite {
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
                "expected `@vary(dimension)`, `@where(dimension=value, ...)`, `@first(dimension)`, `@last(dimension)`, `@same(dimension, ...)`, `@each(dimension, ...)`, `@check(...)`, `@screen(...)` or `@review(...)`"
            }
        }
    }
}

impl ParseError {
    pub(crate) fn of(line: usize, kind: impl Into<ParseErrorKind>) -> Self {
        let mut error = Located::unplaced(ParseFailure {
            kind: Box::new(kind.into()),
        });
        error.location.line = Some(line);
        error
    }

    pub fn line(&self) -> usize {
        self.location.line.unwrap_or_default()
    }

    pub fn kind(&self) -> &ParseErrorKind {
        &self.error.kind
    }

    pub(crate) fn at_token(mut self, token: &str) -> Self {
        self.location
            .focus
            .get_or_insert_with(|| Focus::Address(address_of(token)));
        self
    }

    pub(crate) fn within(mut self, place: &Place) -> Self {
        if self.location.columns.is_none() && self.location.focus.is_none() {
            self.location.columns = Some(place.columns.clone());
        }
        self
    }

    pub(crate) fn locate(mut self, line: &str) -> Self {
        if self.location.columns.is_none() {
            if let Some(columns) = match &self.location.focus {
                Some(Focus::Address(address)) if line.contains('\n') => columns_at(line, address),
                _ => None,
            } {
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

#[cfg(test)]
pub(crate) fn assert_worded<E: std::fmt::Debug + std::fmt::Display>(
    variants: impl IntoIterator<Item = E>,
) {
    for variant in variants {
        let text = variant.to_string();
        assert!(!text.is_empty(), "{variant:?} has no wording");
        let debug = format!("{variant:?}");
        for held in debug.split('"').skip(1).step_by(2) {
            assert!(
                text.contains(held),
                "{variant:?} words {text:?} without `{held}`"
            );
        }
    }
}
