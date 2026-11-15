//! Errors the parser reads from `inventory`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `inventory`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InventoryError {
    /// Says: `{word}` comes before every section of a .spitout
    RecordBeforeSection { word: String },
    /// Says: a .spitout names its {what} once
    SpitoutNamesOnce { what: String },
    /// Says: named context section `{name}` is replaced by `contexts:`
    NamedContextsReplaced { name: String },
    /// Says: expected source path: product: template
    ExpectedSourcePath,
    /// Says: duplicate source path rule for `{name}`
    DuplicateSourcePathRule { name: String },
    /// Says: source list needs a parent context
    SourceListWithoutContext,
    /// Says: source list must be indented under its context
    SourceListNotIndented,
    /// Says: expected inventory section header: sources:, contexts:, or contexts name:
    ExpectedSectionHeader,
    /// Says: `{key}:` needs the removed artifact or group above it
    KeyWithoutRemovedRecord { key: String },
    /// Says: `found:` must be a nonnegative integer
    FoundMustNonnegativeInteger,
    /// Says: a verdict needs a `verdict: pass` or `verdict: fail` line and an `at: file line N` line
    VerdictIncomplete,
    /// Says: `{key}:` needs the review and what it judges above it
    NeedsReviewWhatJudgesAbove { key: String },
    /// Says: `verdict:` must be `pass` or `fail`
    VerdictMustPassFail,
    /// Says: `at:` must be a file and a line, as `qc/looked_at.csv line 2`
    MustFileLineQcLookedCsv,
    /// Says: expected source artifact: product\[dimension=value,...\]
    ExpectedSourceArtifact,
    /// Says: a source record names no file; its source's path rule gives it, so remove `{path}`
    SourceRecordNamesNoFile { path: String },
    /// Says: expected at least one source product
    ExpectedLeastSourceProduct,
    /// Says: expected nested group: \[dimension=value,...\]:
    ExpectedNestedGroup,
    /// Says: nested group repeats dimension `{dimension}`
    NestedGroupRepeatsDimension { dimension: String },
    /// Says: duplicate nested value `{value}`
    DuplicateNestedValue { value: String },
    /// Says: nested group needs a value
    NestedGroupNeedsValue,
    /// Says: expected context: \[dimension=value,...\]
    ExpectedContext,
    /// Says: expected closing `\]` in source artifact
    UnclosedSourceArtifact,
    /// Says: expected `dimension=value` in source artifact
    ExpectedBindingInArtifact,
    /// Says: source dimension value must be one nonempty token
    ArtifactValueNotOneToken,
    /// Says: duplicate source dimension `{dimension}`
    DuplicateSourceDimension { dimension: String },
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecordBeforeSection { word } => write!(f, "`{word}` comes before every section of a .spitout"),
            Self::SpitoutNamesOnce { what } => write!(f, "a .spitout names its {what} once"),
            Self::NamedContextsReplaced { name } => write!(f, "named context section `{name}` is replaced by `contexts:`"),
            Self::ExpectedSourcePath => f.write_str("expected source path: product: template"),
            Self::DuplicateSourcePathRule { name } => write!(f, "duplicate source path rule for `{name}`"),
            Self::SourceListWithoutContext => f.write_str("source list needs a parent context"),
            Self::SourceListNotIndented => f.write_str("source list must be indented under its context"),
            Self::ExpectedSectionHeader => f.write_str("expected inventory section header: sources:, contexts:, or contexts name:"),
            Self::KeyWithoutRemovedRecord { key } => write!(f, "`{key}:` needs the removed artifact or group above it"),
            Self::FoundMustNonnegativeInteger => f.write_str("`found:` must be a nonnegative integer"),
            Self::VerdictIncomplete => f.write_str("a verdict needs a `verdict: pass` or `verdict: fail` line and an `at: file line N` line"),
            Self::NeedsReviewWhatJudgesAbove { key } => write!(f, "`{key}:` needs the review and what it judges above it"),
            Self::VerdictMustPassFail => f.write_str("`verdict:` must be `pass` or `fail`"),
            Self::MustFileLineQcLookedCsv => f.write_str("`at:` must be a file and a line, as `qc/looked_at.csv line 2`"),
            Self::ExpectedSourceArtifact => f.write_str("expected source artifact: product[dimension=value,...]"),
            Self::SourceRecordNamesNoFile { path } => write!(f, "a source record names no file; its source's path rule gives it, so remove `{path}`"),
            Self::ExpectedLeastSourceProduct => f.write_str("expected at least one source product"),
            Self::ExpectedNestedGroup => f.write_str("expected nested group: [dimension=value,...]:"),
            Self::NestedGroupRepeatsDimension { dimension } => write!(f, "nested group repeats dimension `{dimension}`"),
            Self::DuplicateNestedValue { value } => write!(f, "duplicate nested value `{value}`"),
            Self::NestedGroupNeedsValue => f.write_str("nested group needs a value"),
            Self::ExpectedContext => f.write_str("expected context: [dimension=value,...]"),
            Self::UnclosedSourceArtifact => f.write_str("expected closing `]` in source artifact"),
            Self::ExpectedBindingInArtifact => f.write_str("expected `dimension=value` in source artifact"),
            Self::ArtifactValueNotOneToken => f.write_str("source dimension value must be one nonempty token"),
            Self::DuplicateSourceDimension { dimension } => write!(f, "duplicate source dimension `{dimension}`"),
        }
    }
}
