//! An operation written with a body: its header ends in `:`, and the steps
//! indented beneath it carry it out in place of a command.

use crate::model::{BodyStep, OperationDef};
use crate::span::Place;

use super::flow::parse_flow_step;
use super::keyword::Keyword;
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
    /// The indentation of the header.
    header: usize,
    /// The indentation the body's steps share, once the first is read.
    body: Option<usize>,
}

impl OpenBody {
    /// Open the body of `operation`, declared on line `number`, `original`,
    /// at `indent`.
    pub(super) fn open(
        operation: OperationDef,
        place: Place,
        stage: Option<String>,
        original: &str,
        number: usize,
        indent: usize,
    ) -> Self {
        Self {
            operation,
            place,
            stage,
            text: original.to_owned(),
            number,
            header: indent,
            body: None,
        }
    }

    /// Whether a line indented by `indent` belongs to the body.
    pub(super) fn holds(&self, indent: usize) -> bool {
        indent > self.header
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
        match self.body {
            None => self.body = Some(indent),
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
        if Keyword::split(line).is_some() || !line.contains('=') {
            return Err(ParseError::new(
                number,
                format!("the body of operation `{name}` holds only steps, each `output = operation(inputs)`; declare operations, checks and paths outside it"),
            ));
        }
        let (invocation, outputs) = parse_flow_step(line, number)?;
        let place = step_place(original, number, &invocation).call();
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
    pub(super) fn close(self, syntax: &mut Syntax) -> Result<(), ParseError> {
        if self.operation.steps.is_empty() {
            return Err(ParseError::new(
                self.number,
                format!(
                    "operation `{}` ends its header with `:` but has no steps; indent each beneath it as `output = operation(inputs)`, or drop the `:` and give it a `command`",
                    self.operation.name
                ),
            )
            .with_kind(ParseErrorKind::EmptyBody));
        }
        syntax.push(
            &self.text,
            self.number,
            StatementKind::Operation(self.operation, self.place, self.stage),
        );
        Ok(())
    }
}
