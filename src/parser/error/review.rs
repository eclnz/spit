//! The errors of `review` declarations and the review lists; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `review`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    /// Reads: a review takes no command, because a person judges it; write `review {name}` in the pipeline, and name its verdicts in the recipe with `review {name} from file.csv`
    ReviewHasCommand { name: String },
    /// Reads: a review takes no parameters, so it takes no parentheses; write `review {name}`
    ReviewHasParameters { name: String },
    /// Reads: expected one file after `review {name} from`, such as `review {name} from qc/{name}.csv`
    ExpectedReviewFile { name: String },
    /// Reads: `@review(...)` names at least one review, as in `@review(looked_at)`
    EmptyReviewClause,
    /// Reads: expected the reviews that judge every output, as in `review: looked_at`
    EmptyDefaultReviews,
    /// Reads: {what} takes `@check(...)` and `@review(...)`, not `@{other}(...)`
    ClauseNotAllowed { what: String, other: String },
    /// Reads: {what} cannot opt out of a default check; `!{used}` belongs on an output
    OptOutOfDefaultCheck { what: String, used: String },
    /// Reads: {what} cannot opt out of a default review; `!{review}` belongs on an output
    OptOutOfDefaultReview { what: String, review: String },
    /// Reads: expected `review name`, or in a recipe `review name from file.csv`
    ExpectedReviewForm,
    /// Reads: a review takes no arguments; write `{bare}`
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

    /// Every variant names each text it holds in its wording.
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

    /// The names of the `String` fields in a variant's debug text, which its
    /// test gives each as its own value.
    fn held_text(debug: &str) -> Vec<&str> {
        debug.split('"').skip(1).step_by(2).collect()
    }
}
