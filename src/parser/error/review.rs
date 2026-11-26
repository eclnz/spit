use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    ExpectedReviewFile { name: String },
    EmptyReviewClause,
    ClauseNotAllowed { what: String, other: String },
    OptOutOfDefaultCheck { what: String, used: String },
    ExpectedReviewForm,
    ReviewOnInput,
    ReviewAfterCall,
    ReviewRepeated,
    DuplicateEvidence,
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedReviewFile { name } => write!(f, "expected one file after `review {name} from`, such as `review {name} from qc/{name}.csv`"),
            Self::EmptyReviewClause => f.write_str("list the evidence products in `@review(...)` or write bare `@review` to inspect each output directly"),
            Self::ClauseNotAllowed { what, other } => write!(f, "{what} takes `@check(...)` and `@screen(...)`, not `@{other}(...)`"),
            Self::OptOutOfDefaultCheck { what, used } => write!(f, "{what} cannot opt out of a default check; `!{used}` belongs on an output"),
            Self::ExpectedReviewForm => f.write_str("expected `review product from file.csv` in the recipe, or `@review` after a call"),
            Self::ReviewOnInput | Self::ReviewAfterCall => f.write_str("write `@review` or `@review(evidence_product)` after the call that makes the reviewed product"),
            Self::ReviewRepeated => f.write_str("write one `@review` clause after the call"),
            Self::DuplicateEvidence => f.write_str("list each evidence product once"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            ReviewError::ExpectedReviewFile {
                name: "name".to_owned(),
            },
            ReviewError::EmptyReviewClause,
            ReviewError::ClauseNotAllowed {
                what: "what".to_owned(),
                other: "other".to_owned(),
            },
            ReviewError::OptOutOfDefaultCheck {
                what: "what".to_owned(),
                used: "used".to_owned(),
            },
            ReviewError::ExpectedReviewForm,
            ReviewError::ReviewOnInput,
            ReviewError::ReviewAfterCall,
            ReviewError::ReviewRepeated,
            ReviewError::DuplicateEvidence,
        ];
        crate::parser::error::assert_worded(variants);
    }
}
