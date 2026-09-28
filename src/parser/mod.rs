//! Parse sectioned or flow-style pipelines and separate source inventories.

mod declarations;
mod flow;
mod inventory;
mod lexical;
mod operation;
mod sectioned;
mod source_map;

use std::fmt;

use crate::model::{CommandDef, CoverageRule, Invocation, OperationDef, ProductDef};
use crate::paths::PathTemplate;
use crate::span::{content_columns, Focus, Located, Place};
use crate::types::TypeExpr;

use self::flow::parse_flow;
use self::sectioned::{is_sectioned_document, parse_sectioned};

pub(crate) use self::declarations::{parse_use, UseSpec};
pub(crate) use self::inventory::split_document;
pub use self::inventory::{parse_source_inventory, render_source_inventory};
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
        let start = token.as_ptr() as usize;
        self.location
            .focus
            .get_or_insert(Focus::Slice(start..start + token.len()));
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
            let base = line.as_ptr() as usize;
            let token = match self.location.focus.take() {
                Some(Focus::Slice(token)) => Some(token),
                _ => None,
            };
            let token = token.and_then(|token| {
                let start = token.start.checked_sub(base)?;
                let end = token.end.checked_sub(base)?;
                (end <= line.len()).then_some(start..end)
            });
            self.location.columns = Some(token.unwrap_or_else(|| content_columns(line)));
        }
        self
    }
}

/// A pipeline as written: its statements in source order, before lowering
/// turns them into a [`Pipeline`]. Each keeps where its parts sit.
#[derive(Clone, Debug, Default)]
pub(crate) struct Syntax {
    pub(crate) statements: Vec<Statement>,
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

/// Whether to read a document's inline inventory. A separate inventory
/// replaces it, so it is then skipped rather than required to parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InlineInventory {
    Read,
    Skip,
}

/// Parse a pipeline's statements, in the sectioned or the flow form.
pub(crate) fn parse_syntax(text: &str) -> Result<Syntax, ParseError> {
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
