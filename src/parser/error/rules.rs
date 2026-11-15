//! The errors of a recipe's `require` and `exclude` rules; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `rules`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleError {
    /// Reads: `drop` starts no rule; a conditional `exclude` removes the groups that meet a condition: write `exclude {rest}`
    DropReplaced { rest: String },
    /// Reads: expected values after `has`, such as `has run=1,2`
    ExpectedValuesAfterHas,
    /// Reads: write the count before `has`, as in `require \[sub\] where bold count>=2 has run=1,2`
    CountBeforeHas,
    /// Reads: required values follow `has`: write `has {token}`
    ValuesFollowHas { token: String },
    /// Reads: expected `where` after the groups, as in `{example}`
    ExpectedWhereAfterGroups { example: String },
    /// Reads: expected `\]` after context dimensions
    UnclosedContextDimensions,
    /// Reads: context target dimensions must be distinct
    ContextDimensionsRepeat,
    /// Reads: expected values after `{condition}`, such as `{condition} run=1,2`
    ExpectedValuesAfterCondition { condition: String },
    /// Reads: a conditional `exclude` rule takes one condition
    ExcludeNeedsOneCondition,
    /// Reads: `require` names the groups first, then `where`: write `{rule}`
    RequireGroupsFirst { rule: String },
    /// Reads: a rule takes one count
    RuleTakesCount,
    /// Reads: required values must not be empty
    RequiredValuesEmpty,
    /// Reads: duplicate required dimension `{dimension}`
    DuplicateRequiredDimension { dimension: String },
    /// Reads: expected `\[` before constraint dimensions
    ExpectedOpenBracket,
    /// Reads: expected closing `\]` in constraint dimensions
    UnclosedConstraintDimensions,
    /// Reads: constraint count must be a nonnegative integer
    CountNotInteger,
    /// A coverage rule that does not read, with the form it is told to write.
    ExpectedForm { form: RuleForm },
    /// Reads: conditional `exclude` names the groups first, then `where`: write `{start} {count}`
    ExcludeGroupsFirstCount { start: String, count: String },
    /// Reads: conditional `exclude` names the groups first, then `where`: write `{start} has {values}` to remove the groups that have them, or `{start} missing {values}` to remove the groups that lack one
    ExcludeGroupsFirstValues { start: String, values: String },
    /// Reads: conditional `exclude` names the groups first, then `where`, as in `exclude \[sub\] where sessions count<2`
    ExcludeGroupsFirst,
    /// Reads: `skip` starts no rule; a conditional `exclude` names the groups to remove: write `{rule}`
    SkipReplaced { rule: String },
    /// Reads: `skip` starts no rule; a conditional `exclude` names the groups to remove, as in `exclude \[sub\] where sessions count<2`
    SkipReplacedWithoutRule,
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
            Self::RequireGroupsFirst { rule } => write!(f, "`require` names the groups first, then `where`: write `{rule}`"),
            Self::RuleTakesCount => f.write_str("a rule takes one count"),
            Self::RequiredValuesEmpty => f.write_str("required values must not be empty"),
            Self::DuplicateRequiredDimension { dimension } => write!(f, "duplicate required dimension `{dimension}`"),
            Self::ExpectedOpenBracket => f.write_str("expected `[` before constraint dimensions"),
            Self::UnclosedConstraintDimensions => f.write_str("expected closing `]` in constraint dimensions"),
            Self::CountNotInteger => f.write_str("constraint count must be a nonnegative integer"),
            Self::ExpectedForm { form } => write!(f, "{form}"),
            Self::ExcludeGroupsFirstCount { start, count } => write!(f, "conditional `exclude` names the groups first, then `where`: write `{start} {count}`"),
            Self::ExcludeGroupsFirstValues { start, values } => write!(f, "conditional `exclude` names the groups first, then `where`: write `{start} has {values}` to remove the groups that have them, or `{start} missing {values}` to remove the groups that lack one"),
            Self::ExcludeGroupsFirst => f.write_str("conditional `exclude` names the groups first, then `where`, as in `exclude [sub] where sessions count<2`"),
            Self::SkipReplaced { rule } => write!(f, "`skip` starts no rule; a conditional `exclude` names the groups to remove: write `{rule}`"),
            Self::SkipReplacedWithoutRule => f.write_str("`skip` starts no rule; a conditional `exclude` names the groups to remove, as in `exclude [sub] where sessions count<2`"),
        }
    }
}

/// The form a coverage rule is written in, which says what a rule that does
/// not read is told to write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleForm {
    /// `require [dimensions] where source`, then a count, `has`, or both.
    Require,
    /// `require [dimensions] where source`, as the text of a rule written in another order.
    RequireGroups,
    /// `exclude [dimensions] where source` and one condition.
    Exclude,
    /// `exclude [dimensions] where source`, as the text of a rule written in another order.
    ExcludeGroups,
    /// `skip product count>=2 per [dimensions]`.
    Skip,
}

impl fmt::Display for RuleForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Require => "expected `require [dimensions] where source` and a condition: `count=1`, `has run=1,2`, or both",
            Self::RequireGroups => "expected `require [dimensions] where source`",
            Self::Exclude => "expected `exclude [dimensions] where source` and one condition: `count<2`, `missing run=1,2` or `has run=3`",
            Self::ExcludeGroups => "expected `exclude [dimensions] where source`",
            Self::Skip => "expected `skip product count>=2 per [dimensions]`",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant names each text it holds in its wording.
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
            RuleError::RequireGroupsFirst {
                rule: "rule".to_owned(),
            },
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
            RuleError::ExcludeGroupsFirstCount {
                start: "start".to_owned(),
                count: "count".to_owned(),
            },
            RuleError::ExcludeGroupsFirstValues {
                start: "start".to_owned(),
                values: "values".to_owned(),
            },
            RuleError::ExcludeGroupsFirst,
            RuleError::SkipReplaced {
                rule: "rule".to_owned(),
            },
            RuleError::SkipReplacedWithoutRule,
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
