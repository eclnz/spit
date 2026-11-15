//! Errors the parser reads from `inventory`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `inventory`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InventoryError {
    /// Says: `{word}` comes before every section of a .spitout
    ComesBeforeEverySectionSpitout { word: String },
    /// Says: a .spitout names its {what} once
    SpitoutNamesOnce { what: String },
    /// Says: named context section `{name}` is replaced by `contexts:`
    NamedContextSectionReplacedContexts { name: String },
    /// Says: expected source path: product: template
    ExpectedSourcePathProductTemplate,
    /// Says: duplicate source path rule for `{name}`
    DuplicateSourcePathRule { name: String },
    /// Says: source list needs a parent context
    SourceListNeedsParentContext,
    /// Says: source list must be indented under its context
    SourceListMustIndentedUnderContext,
    /// Says: expected inventory section header: sources:, contexts:, or contexts name:
    ExpectedInventorySectionHeaderSourcesContexts,
    /// Says: `{key}:` needs the removed artifact or group above it
    NeedsRemovedArtifactGroupAbove { key: String },
    /// Says: `found:` must be a nonnegative integer
    FoundMustNonnegativeInteger,
    /// Says: a verdict needs a `verdict: pass` or `verdict: fail` line and an `at: file line N` line
    VerdictNeedsVerdictPassVerdictFail,
    /// Says: `{key}:` needs the review and what it judges above it
    NeedsReviewWhatJudgesAbove { key: String },
    /// Says: `verdict:` must be `pass` or `fail`
    VerdictMustPassFail,
    /// Says: `at:` must be a file and a line, as `qc/looked_at.csv line 2`
    MustFileLineQcLookedCsv,
    /// Says: expected source artifact: product\[dimension=value,...\]
    ExpectedSourceArtifactProductDimensionValue,
    /// Says: a source record names no file; its source's path rule gives it, so remove `{path}`
    SourceRecordNamesNoFileSource { path: String },
    /// Says: expected at least one source product
    ExpectedLeastSourceProduct,
    /// Says: expected nested group: \[dimension=value,...\]:
    ExpectedNestedGroupDimensionValue,
    /// Says: nested group repeats dimension `{dimension}`
    NestedGroupRepeatsDimension { dimension: String },
    /// Says: duplicate nested value `{value}`
    DuplicateNestedValue { value: String },
    /// Says: nested group needs a value
    NestedGroupNeedsValue,
    /// Says: expected context: \[dimension=value,...\]
    ExpectedContextDimensionValue,
    /// Says: expected closing `\]` in source artifact
    ExpectedClosingSourceArtifact,
    /// Says: expected `dimension=value` in source artifact
    ExpectedDimensionValueSourceArtifact,
    /// Says: source dimension value must be one nonempty token
    SourceDimensionValueMustNonemptyToken,
    /// Says: duplicate source dimension `{dimension}`
    DuplicateSourceDimension { dimension: String },
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComesBeforeEverySectionSpitout { word } => write!(f, "`{word}` comes before every section of a .spitout"),
            Self::SpitoutNamesOnce { what } => write!(f, "a .spitout names its {what} once"),
            Self::NamedContextSectionReplacedContexts { name } => write!(f, "named context section `{name}` is replaced by `contexts:`"),
            Self::ExpectedSourcePathProductTemplate => f.write_str("expected source path: product: template"),
            Self::DuplicateSourcePathRule { name } => write!(f, "duplicate source path rule for `{name}`"),
            Self::SourceListNeedsParentContext => f.write_str("source list needs a parent context"),
            Self::SourceListMustIndentedUnderContext => f.write_str("source list must be indented under its context"),
            Self::ExpectedInventorySectionHeaderSourcesContexts => f.write_str("expected inventory section header: sources:, contexts:, or contexts name:"),
            Self::NeedsRemovedArtifactGroupAbove { key } => write!(f, "`{key}:` needs the removed artifact or group above it"),
            Self::FoundMustNonnegativeInteger => f.write_str("`found:` must be a nonnegative integer"),
            Self::VerdictNeedsVerdictPassVerdictFail => f.write_str("a verdict needs a `verdict: pass` or `verdict: fail` line and an `at: file line N` line"),
            Self::NeedsReviewWhatJudgesAbove { key } => write!(f, "`{key}:` needs the review and what it judges above it"),
            Self::VerdictMustPassFail => f.write_str("`verdict:` must be `pass` or `fail`"),
            Self::MustFileLineQcLookedCsv => f.write_str("`at:` must be a file and a line, as `qc/looked_at.csv line 2`"),
            Self::ExpectedSourceArtifactProductDimensionValue => f.write_str("expected source artifact: product[dimension=value,...]"),
            Self::SourceRecordNamesNoFileSource { path } => write!(f, "a source record names no file; its source's path rule gives it, so remove `{path}`"),
            Self::ExpectedLeastSourceProduct => f.write_str("expected at least one source product"),
            Self::ExpectedNestedGroupDimensionValue => f.write_str("expected nested group: [dimension=value,...]:"),
            Self::NestedGroupRepeatsDimension { dimension } => write!(f, "nested group repeats dimension `{dimension}`"),
            Self::DuplicateNestedValue { value } => write!(f, "duplicate nested value `{value}`"),
            Self::NestedGroupNeedsValue => f.write_str("nested group needs a value"),
            Self::ExpectedContextDimensionValue => f.write_str("expected context: [dimension=value,...]"),
            Self::ExpectedClosingSourceArtifact => f.write_str("expected closing `]` in source artifact"),
            Self::ExpectedDimensionValueSourceArtifact => f.write_str("expected `dimension=value` in source artifact"),
            Self::SourceDimensionValueMustNonemptyToken => f.write_str("source dimension value must be one nonempty token"),
            Self::DuplicateSourceDimension { dimension } => write!(f, "duplicate source dimension `{dimension}`"),
        }
    }
}
