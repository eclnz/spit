//! Errors the parser reads from `review`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `review`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    /// Says: a review takes no command, because a person judges it; write `review {name}` in the pipeline, and name its verdicts in the recipe with `review {name} from file.csv`
    ReviewHasCommand { name: String },
    /// Says: a review takes no parameters, so it takes no parentheses; write `review {name}`
    ReviewHasParameters { name: String },
    /// Says: expected one file after `review {name} from`, such as `review {name} from qc/{name}.csv`
    ExpectedReviewFile { name: String },
    /// Says: `@review(...)` names at least one review, as in `@review(looked_at)`
    EmptyReviewClause,
    /// Says: expected the reviews that judge every output, as in `review: looked_at`
    EmptyDefaultReviews,
    /// Says: {what} takes `@check(...)` and `@review(...)`, not `@{other}(...)`
    TakesCheckReviewNot { what: String, other: String },
    /// Says: {what} cannot opt out of a default check; `!{used}` belongs on an output
    OptOutOfDefaultCheck { what: String, used: String },
    /// Says: {what} cannot opt out of a default review; `!{review}` belongs on an output
    OptOutOfDefaultReview { what: String, review: String },
    /// Says: expected `review name`, or in a recipe `review name from file.csv`
    ExpectedReviewForm,
    /// Says: a review takes no arguments; write `{bare}`
    ReviewTakesNoArgumentsWrite { bare: String },
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReviewHasCommand { name } => write!(f, "a review takes no command, because a person judges it; write `review {name}` in the pipeline, and name its verdicts in the recipe with `review {name} from file.csv`"),
            Self::ReviewHasParameters { name } => write!(f, "a review takes no parameters, so it takes no parentheses; write `review {name}`"),
            Self::ExpectedReviewFile { name } => write!(f, "expected one file after `review {name} from`, such as `review {name} from qc/{name}.csv`"),
            Self::EmptyReviewClause => f.write_str("`@review(...)` names at least one review, as in `@review(looked_at)`"),
            Self::EmptyDefaultReviews => f.write_str("expected the reviews that judge every output, as in `review: looked_at`"),
            Self::TakesCheckReviewNot { what, other } => write!(f, "{what} takes `@check(...)` and `@review(...)`, not `@{other}(...)`"),
            Self::OptOutOfDefaultCheck { what, used } => write!(f, "{what} cannot opt out of a default check; `!{used}` belongs on an output"),
            Self::OptOutOfDefaultReview { what, review } => write!(f, "{what} cannot opt out of a default review; `!{review}` belongs on an output"),
            Self::ExpectedReviewForm => f.write_str("expected `review name`, or in a recipe `review name from file.csv`"),
            Self::ReviewTakesNoArgumentsWrite { bare } => write!(f, "a review takes no arguments; write `{bare}`"),
        }
    }
}
