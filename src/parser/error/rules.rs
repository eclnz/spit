//! Errors the parser reads from `rules`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `rules`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleError {
    /// Says: `drop` starts no rule; a conditional `exclude` removes the groups that meet a condition: write `exclude {rest}`
    DropReplaced { rest: String },
    /// Says: expected values after `has`, such as `has run=1,2`
    ExpectedValuesAfterHas,
    /// Says: write the count before `has`, as in `require \[sub\] where bold count>=2 has run=1,2`
    CountBeforeHas,
    /// Says: required values follow `has`: write `has {token}`
    ValuesFollowHas { token: String },
    /// Says: expected `where` after the groups, as in `{example}`
    ExpectedWhereAfterGroups { example: String },
    /// Says: expected `\]` after context dimensions
    UnclosedContextDimensions,
    /// Says: context target dimensions must be distinct
    ContextDimensionsRepeat,
    /// Says: expected values after `{condition}`, such as `{condition} run=1,2`
    ExpectedValuesAfterSuchRun { condition: String },
    /// Says: a conditional `exclude` rule takes one condition
    ExcludeNeedsOneCondition,
    /// Says: `require` names the groups first, then `where`: write `{rule}`
    RequireGroupsFirst { rule: String },
    /// Says: a rule takes one count
    RuleTakesCount,
    /// Says: required values must not be empty
    RequiredValuesMustNotEmpty,
    /// Says: duplicate required dimension `{dimension}`
    DuplicateRequiredDimension { dimension: String },
    /// Says: expected `\[` before constraint dimensions
    ExpectedOpenBracket,
    /// Says: expected closing `\]` in constraint dimensions
    UnclosedConstraintDimensions,
    /// Says: constraint count must be a nonnegative integer
    CountNotInteger,
    /// Says: {form}
    ExpectedForm { form: RuleForm },
    /// Says: conditional `exclude` names the groups first, then `where`: write `{start} {count}`
    ExcludeGroupsFirstCount { start: String, count: String },
    /// Says: conditional `exclude` names the groups first, then `where`: write `{start} has {values}` to remove the groups that have them, or `{start} missing {values}` to remove the groups that lack one
    ExcludeGroupsFirstValues { start: String, values: String },
    /// Says: conditional `exclude` names the groups first, then `where`, as in `exclude \[sub\] where sessions count<2`
    ExcludeGroupsFirst,
    /// Says: `skip` starts no rule; a conditional `exclude` names the groups to remove: write `{rule}`
    SkipReplaced { rule: String },
    /// Says: `skip` starts no rule; a conditional `exclude` names the groups to remove, as in `exclude \[sub\] where sessions count<2`
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
            Self::ExpectedValuesAfterSuchRun { condition } => write!(f, "expected values after `{condition}`, such as `{condition} run=1,2`"),
            Self::ExcludeNeedsOneCondition => f.write_str("a conditional `exclude` rule takes one condition"),
            Self::RequireGroupsFirst { rule } => write!(f, "`require` names the groups first, then `where`: write `{rule}`"),
            Self::RuleTakesCount => f.write_str("a rule takes one count"),
            Self::RequiredValuesMustNotEmpty => f.write_str("required values must not be empty"),
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
