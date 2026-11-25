use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyError {
    BodyHoldsOnlySteps { name: String },
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
        crate::parser::error::assert_worded(variants);
    }
}
