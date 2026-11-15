//! Errors the parser reads from `check`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `check`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckError {
    /// Says: expected check name(parameters): executable {@path} \[arguments\]
    ExpectedCheckForm,
    /// Says: check `{check}` repeats parameter `{parameter}`
    RepeatedCheckParameter { check: String, parameter: String },
    /// Says: check `{check}` takes no parameters, so it takes no parentheses; write `check {check}:`
    CheckParenthesesWithoutParameters { check: String },
    /// Says: a check runs one command, written after its `:`, as in `check {check}: test -s {@path}`
    CheckCommandOnNextLine { check: String },
    /// Says: check command must not be empty
    EmptyCheckCommand,
    /// Says: write `@{keyword}(...)`, with no space between `@` and `{keyword}`
    SpaceAfterAt { keyword: String },
    /// Says: `@check(...)` names at least one check, as in `@check(nonempty)`
    EmptyCheckClause,
    /// Says: expected the checks to run on every output, as in `check: nonempty, ndim(3)`
    EmptyDefaultChecks,
    /// Says: check `{check}` is given no arguments; write `{check}` without parentheses
    CheckWithoutArguments { check: String },
    /// Says: `{argument}` cannot be a check argument; give one word without spaces, quotes, braces or parentheses, as in `ndim(3)`
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
