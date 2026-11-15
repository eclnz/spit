//! Errors the parser reads from `body`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `body`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyError {
    /// Says: the body of operation `{name}` holds only steps, each `output = operation(inputs)`; declare operations, checks and paths outside it
    BodyOperationHoldsOnlyStepsOutput { name: String },
    /// Says: this step is indented differently from the other steps of operation `{name}`
    StepIndentedDifferentlyOtherStepsOperation { name: String },
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyOperationHoldsOnlyStepsOutput { name } => write!(f, "the body of operation `{name}` holds only steps, each `output = operation(inputs)`; declare operations, checks and paths outside it"),
            Self::StepIndentedDifferentlyOtherStepsOperation { name } => write!(f, "this step is indented differently from the other steps of operation `{name}`"),
        }
    }
}
