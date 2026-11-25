use crate::model::{BodyStep, OperationDef};
use crate::span::{content_columns, Place};

use super::error::BodyError;
use super::flow::parse_flow_step;
use super::keyword::Keyword;
use super::layout::is_step;
use super::source_map::step_place;
use super::{ParseError, ParseErrorKind, StatementKind, Syntax};

pub(super) struct OpenBody {
    operation: OperationDef,
    place: Place,
    stage: Option<String>,
    text: String,
    number: usize,
    body: Option<usize>,
}

impl OpenBody {
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

    pub(super) fn line(
        &mut self,
        original: &str,
        line: &str,
        number: usize,
        indent: usize,
    ) -> Result<(), ParseError> {
        let name = &self.operation.name;
        if !is_step(line) && (Keyword::split(line).is_some() || !line.contains('=')) {
            return Err(ParseError::of(
                number,
                BodyError::BodyHoldsOnlySteps {
                    name: name.to_string(),
                },
            ));
        }
        match self.body {
            None => {}
            Some(body) if body == indent => {}
            Some(_) => {
                return Err(ParseError::of(
                    number,
                    BodyError::StepIndentedDifferently {
                        name: name.to_string(),
                    },
                ))
            }
        }
        let (invocation, outputs) = parse_flow_step(line, number)?;
        let place = step_place(original, number, &invocation).call();
        self.body = Some(indent);
        self.operation.steps.push(BodyStep {
            invocation,
            outputs,
            place,
        });
        Ok(())
    }

    pub(super) fn header(&self) -> &str {
        &self.text
    }

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

pub(crate) fn empty_body(name: &str, number: usize) -> ParseError {
    ParseError::of(
        number,
        ParseErrorKind::EmptyBody {
            name: name.to_owned(),
        },
    )
}
