use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleError {
    DropReplaced { rest: String },
    ExpectedValuesAfterHas,
    CountBeforeHas,
    ValuesFollowHas { token: String },
    ExpectedWhereAfterGroups { example: String },
    UnclosedContextDimensions,
    ContextDimensionsRepeat,
    ExpectedValuesAfterCondition { condition: String },
    ExcludeNeedsOneCondition,
    RuleTakesCount,
    RequiredValuesEmpty,
    DuplicateRequiredDimension { dimension: String },
    ExpectedOpenBracket,
    UnclosedConstraintDimensions,
    CountNotInteger,
    ExpectedForm { form: RuleForm },
    SkipReplaced,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DropReplaced { rest } => write!(f, "`drop` starts no rule; a conditional `exclude` removes the groups that meet a condition: write `exclude {rest}`"),
            Self::ExpectedValuesAfterHas => f.write_str("expected values after `has`, such as `has run=1,2`"),
            Self::CountBeforeHas => f.write_str("write the count before `has`, as in `require [sub] where bold count>=2 has run=1,2`"),
            Self::ValuesFollowHas { token } => write!(f, "required values follow `has`: write `has {token}`"),
            Self::ExpectedWhereAfterGroups { example } => write!(f, "expected `where` after the groups, as in `{example}`"),
            Self::UnclosedContextDimensions => f.write_str("expected `]` after context dimensions"),
            Self::ContextDimensionsRepeat => f.write_str("context target dimensions must be distinct"),
            Self::ExpectedValuesAfterCondition { condition } => write!(f, "expected values after `{condition}`, such as `{condition} run=1,2`"),
            Self::ExcludeNeedsOneCondition => f.write_str("a conditional `exclude` rule takes one condition"),
            Self::RuleTakesCount => f.write_str("a rule takes one count"),
            Self::RequiredValuesEmpty => f.write_str("required values must not be empty"),
            Self::DuplicateRequiredDimension { dimension } => write!(f, "duplicate required dimension `{dimension}`"),
            Self::ExpectedOpenBracket => f.write_str("expected `[` before constraint dimensions"),
            Self::UnclosedConstraintDimensions => f.write_str("expected closing `]` in constraint dimensions"),
            Self::CountNotInteger => f.write_str("constraint count must be a nonnegative integer"),
            Self::ExpectedForm { form } => write!(f, "{form}"),
            Self::SkipReplaced => f.write_str("`skip` starts no rule; a conditional `exclude` names the groups to remove, as in `exclude [sub] where sessions count<2`"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleForm {
    Require,
    Exclude,
}

impl fmt::Display for RuleForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Require => "expected `require [dimensions] where source` and a condition: `count=1`, `has run=1,2`, or both",
            Self::Exclude => "expected `exclude [dimensions] where source` and one condition: `count<2`, `missing run=1,2` or `has run=3`",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_words_the_text_it_holds() {
        let variants = [
            RuleError::DropReplaced {
                rest: "rest".to_owned(),
            },
            RuleError::ExpectedValuesAfterHas,
            RuleError::CountBeforeHas,
            RuleError::ValuesFollowHas {
                token: "token".to_owned(),
            },
            RuleError::ExpectedWhereAfterGroups {
                example: "example".to_owned(),
            },
            RuleError::UnclosedContextDimensions,
            RuleError::ContextDimensionsRepeat,
            RuleError::ExpectedValuesAfterCondition {
                condition: "condition".to_owned(),
            },
            RuleError::ExcludeNeedsOneCondition,
            RuleError::RuleTakesCount,
            RuleError::RequiredValuesEmpty,
            RuleError::DuplicateRequiredDimension {
                dimension: "dimension".to_owned(),
            },
            RuleError::ExpectedOpenBracket,
            RuleError::UnclosedConstraintDimensions,
            RuleError::CountNotInteger,
            RuleError::ExpectedForm {
                form: RuleForm::Require,
            },
            RuleError::SkipReplaced,
        ];
        crate::parser::error::assert_worded(variants);
    }
}
