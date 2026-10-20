//! An operation written with a body: its header ends in `:`, and the steps
//! indented beneath it carry it out in place of a command.

use crate::model::{BodyStep, OperationDef};
use crate::span::{content_columns, Place};

use super::flow::parse_flow_step;
use super::keyword::Keyword;
use super::layout::is_step;
use super::source_map::step_place;
use super::{ParseError, ParseErrorKind, StatementKind, Syntax};

/// An operation whose body is being read: its header, and the steps read
/// so far.
pub(super) struct OpenBody {
    operation: OperationDef,
    /// Where the operation's name is.
    place: Place,
    /// The stage the header is in, if any.
    stage: Option<String>,
    /// The header line, and its number.
    text: String,
    number: usize,
    /// The indentation the body's steps share, once the first is read.
    body: Option<usize>,
}

impl OpenBody {
    /// Open the body of `operation`, declared on line `number`, `original`.
    pub(super) fn open(
        operation: OperationDef,
        place: Place,
        stage: Option<String>,
        original: &str,
        number: usize,
    ) -> Self {
        Self {
            operation,
            place,
            stage,
            text: original.to_owned(),
            number,
            body: None,
        }
    }

    /// Read a step of the body, `line` being the content of `original`.
    pub(super) fn line(
        &mut self,
        original: &str,
        line: &str,
        number: usize,
        indent: usize,
    ) -> Result<(), ParseError> {
        let name = &self.operation.name;
        // A step whose product takes a keyword's word fails as a step does.
        if !is_step(line) && (Keyword::split(line).is_some() || !line.contains('=')) {
            return Err(ParseError::new(
                number,
                format!("the body of operation `{name}` holds only steps, each `output = operation(inputs)`; declare operations, checks and paths outside it"),
            ));
        }
        match self.body {
            None => {}
            Some(body) if body == indent => {}
            Some(_) => {
                return Err(ParseError::new(
                    number,
                    format!(
                    "this step is indented differently from the other steps of operation `{name}`"
                ),
                ))
            }
        }
        let (invocation, outputs) = parse_flow_step(line, number)?;
        let place = step_place(original, number, &invocation).call();
        // The first step that parses fixes how far the steps are indented.
        self.body = Some(indent);
        self.operation.steps.push(BodyStep {
            invocation,
            outputs,
            place,
        });
        Ok(())
    }

    /// The header line, for locating an error in it.
    pub(super) fn header(&self) -> &str {
        &self.text
    }

    /// End the body, recording the operation once its steps are read.
    /// `ending` is the line that ends it, or none where the text ends.
    pub(super) fn close(self, syntax: &mut Syntax, ending: Option<&str>) -> Result<(), ParseError> {
        let ended = content_columns(ending.unwrap_or(&self.text));
        if self.operation.steps.is_empty() {
            return Err(empty_body(&self.operation.name, self.number)
                .within(&Place::new(self.number, ended)));
        }
        syntax.push(
            &self.text,
            self.number,
            StatementKind::Operation(self.operation, self.place, self.stage, ended),
        );
        Ok(())
    }
}

/// The error for an operation `name`, whose header on line `number` opens
/// a body that holds no step. Keep in step with `lower`, which gives it
/// where each step of a body fails to check.
pub(crate) fn empty_body(name: &str, number: usize) -> ParseError {
    ParseError::new(
        number,
        format!(
            "operation `{name}` ends its header with `:` but has no steps; indent each beneath it as `output = operation(inputs)`, or drop the `:` and give it a `command`"
        ),
    )
    .with_kind(ParseErrorKind::EmptyBody)
}
