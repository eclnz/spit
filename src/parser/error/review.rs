use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    ReviewHasCommand { name: String },
    ReviewHasParameters { name: String },
    ExpectedReviewFile { name: String },
    EmptyReviewClause,
    EmptyDefaultReviews,
    ClauseNotAllowed { what: String, other: String },
    OptOutOfDefaultCheck { what: String, used: String },
    OptOutOfDefaultReview { what: String, review: String },
    ExpectedReviewForm,
    ReviewHasArguments { bare: String },
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReviewHasCommand { name } => write!(f, "a review takes no command, because a person judges it; write `review {name}` in the pipeline, and name its verdicts in the recipe with `review {name} from file.csv`"),
            Self::ReviewHasParameters { name } => write!(f, "a review takes no parameters, so it takes no parentheses; write `review {name}`"),
            Self::ExpectedReviewFile { name } => write!(f, "expected one file after `review {name} from`, such as `review {name} from qc/{name}.csv`"),
            Self::EmptyReviewClause => f.write_str("`@review(...)` names at least one review, as in `@review(looked_at)`"),
            Self::EmptyDefaultReviews => f.write_str("expected the reviews that judge every output, as in `review: looked_at`"),
            Self::ClauseNotAllowed { what, other } => write!(f, "{what} takes `@check(...)` and `@review(...)`, not `@{other}(...)`"),
            Self::OptOutOfDefaultCheck { what, used } => write!(f, "{what} cannot opt out of a default check; `!{used}` belongs on an output"),
            Self::OptOutOfDefaultReview { what, review } => write!(f, "{what} cannot opt out of a default review; `!{review}` belongs on an output"),
            Self::ExpectedReviewForm => f.write_str("expected `review name`, or in a recipe `review name from file.csv`"),
            Self::ReviewHasArguments { bare } => write!(f, "a review takes no arguments; write `{bare}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            ReviewError::ReviewHasCommand {
                name: "name".to_owned(),
            },
            ReviewError::ReviewHasParameters {
                name: "name".to_owned(),
            },
            ReviewError::ExpectedReviewFile {
                name: "name".to_owned(),
            },
            ReviewError::EmptyReviewClause,
            ReviewError::EmptyDefaultReviews,
            ReviewError::ClauseNotAllowed {
                what: "what".to_owned(),
                other: "other".to_owned(),
            },
            ReviewError::OptOutOfDefaultCheck {
                what: "what".to_owned(),
                used: "used".to_owned(),
            },
            ReviewError::OptOutOfDefaultReview {
                what: "what".to_owned(),
                review: "review".to_owned(),
            },
            ReviewError::ExpectedReviewForm,
            ReviewError::ReviewHasArguments {
                bare: "bare".to_owned(),
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

    fn held_text(debug: &str) -> Vec<&str> {
        debug.split('"').skip(1).step_by(2).collect()
    }
}
