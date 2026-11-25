use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckError {
    ExpectedCheckForm,
    RepeatedCheckParameter { check: String, parameter: String },
    CheckParenthesesWithoutParameters { check: String },
    CheckCommandOnNextLine { check: String },
    EmptyCheckCommand,
    SpaceAfterAt { keyword: String },
    EmptyCheckClause,
    EmptyDefaultChecks,
    CheckWithoutArguments { check: String },
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
        crate::parser::error::assert_worded(variants);
    }
}
