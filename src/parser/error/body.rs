//! The errors of the steps of an operation's body; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `body`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyError {
    /// Reads: the body of operation `{name}` holds only steps, each `output = operation(inputs)`; declare operations, checks and paths outside it
    BodyHoldsOnlySteps { name: String },
    /// Reads: this step is indented differently from the other steps of operation `{name}`
    StepIndentedDifferently { name: String },
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyHoldsOnlySteps { name } => write!(f, "the body of operation `{name}` holds only steps, each `output = operation(inputs)`; declare operations, checks and paths outside it"),
            Self::StepIndentedDifferently { name } => write!(f, "this step is indented differently from the other steps of operation `{name}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant names each text it holds in its wording.
    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            BodyError::BodyHoldsOnlySteps {
                name: "name".to_owned(),
            },
            BodyError::StepIndentedDifferently {
                name: "name".to_owned(),
            },
        ];
        for variant in variants {
            let text = variant.to_string();
            assert!(!text.is_empty(), "{variant:?} has no wording");
            for held in held_text(&format!("{variant:?}")) {
                assert!(
                    text.contains(held),
                    "{variant:?} words {text:?} without `{held}`"
                );
            }
        }
    }

    /// The names of the `String` fields in a variant's debug text, which its
    /// test gives each as its own value.
    fn held_text(debug: &str) -> Vec<&str> {
        debug.split('"').skip(1).step_by(2).collect()
    }
}
