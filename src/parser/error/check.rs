//! The errors of `check` declarations and the `@check(...)` and `@review(...)` clauses' check lists; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `check`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckError {
    /// Reads: expected check name(parameters): executable {@path} \[arguments\]
    ExpectedCheckForm,
    /// Reads: check `{check}` repeats parameter `{parameter}`
    RepeatedCheckParameter { check: String, parameter: String },
    /// Reads: check `{check}` takes no parameters, so it takes no parentheses; write `check {check}:`
    CheckParenthesesWithoutParameters { check: String },
    /// Reads: a check runs one command, written after its `:`, as in `check {check}: test -s {@path}`
    CheckCommandOnNextLine { check: String },
    /// Reads: check command must not be empty
    EmptyCheckCommand,
    /// Reads: write `@{keyword}(...)`, with no space between `@` and `{keyword}`
    SpaceAfterAt { keyword: String },
    /// Reads: `@check(...)` names at least one check, as in `@check(nonempty)`
    EmptyCheckClause,
    /// Reads: expected the checks to run on every output, as in `check: nonempty, ndim(3)`
    EmptyDefaultChecks,
    /// Reads: check `{check}` is given no arguments; write `{check}` without parentheses
    CheckWithoutArguments { check: String },
    /// Reads: `{argument}` cannot be a check argument; give one word without spaces, quotes, braces or parentheses, as in `ndim(3)`
    InvalidCheckArgument { argument: String },
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedCheckForm => f.write_str("expected check name(parameters): executable {@path} [arguments]"),
            Self::RepeatedCheckParameter { check, parameter } => write!(f, "check `{check}` repeats parameter `{parameter}`"),
            Self::CheckParenthesesWithoutParameters { check } => write!(f, "check `{check}` takes no parameters, so it takes no parentheses; write `check {check}:`"),
            Self::CheckCommandOnNextLine { check } => write!(f, "a check runs one command, written after its `:`, as in `check {check}: test -s {{@path}}`"),
            Self::EmptyCheckCommand => f.write_str("check command must not be empty"),
            Self::SpaceAfterAt { keyword } => write!(f, "write `@{keyword}(...)`, with no space between `@` and `{keyword}`"),
            Self::EmptyCheckClause => f.write_str("`@check(...)` names at least one check, as in `@check(nonempty)`"),
            Self::EmptyDefaultChecks => f.write_str("expected the checks to run on every output, as in `check: nonempty, ndim(3)`"),
            Self::CheckWithoutArguments { check } => write!(f, "check `{check}` is given no arguments; write `{check}` without parentheses"),
            Self::InvalidCheckArgument { argument } => write!(f, "`{argument}` cannot be a check argument; give one word without spaces, quotes, braces or parentheses, as in `ndim(3)`"),
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
            CheckError::ExpectedCheckForm,
            CheckError::RepeatedCheckParameter {
                check: "check".to_owned(),
                parameter: "parameter".to_owned(),
            },
            CheckError::CheckParenthesesWithoutParameters {
                check: "check".to_owned(),
            },
            CheckError::CheckCommandOnNextLine {
                check: "check".to_owned(),
            },
            CheckError::EmptyCheckCommand,
            CheckError::SpaceAfterAt {
                keyword: "keyword".to_owned(),
            },
            CheckError::EmptyCheckClause,
            CheckError::EmptyDefaultChecks,
            CheckError::CheckWithoutArguments {
                check: "check".to_owned(),
            },
            CheckError::InvalidCheckArgument {
                argument: "argument".to_owned(),
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
