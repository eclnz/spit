//! The errors of a `.spitout`'s sections and records; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `inventory`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InventoryError {
    /// Reads: `{word}` comes before every section of a .spitout
    RootAfterSection { word: String },
    /// Reads: a .spitout names its {what} once
    SpitoutNamesOnce { what: String },
    /// Reads: named context section `{name}` is replaced by `contexts:`
    NamedContextsReplaced { name: String },
    /// Reads: expected source path: product: template
    ExpectedSourcePath,
    /// Reads: duplicate source path rule for `{name}`
    DuplicateSourcePathRule { name: String },
    /// Reads: source list needs a parent context
    SourceListWithoutContext,
    /// Reads: source list must be indented under its context
    SourceListNotIndented,
    /// Reads: expected inventory section header: sources:, contexts:, or contexts name:
    ExpectedSectionHeader,
    /// Reads: `{key}:` needs the removed artifact or group above it
    KeyWithoutRemovedRecord { key: String },
    /// Reads: `found:` must be a nonnegative integer
    FoundNotInteger,
    /// Reads: a verdict needs a `verdict: pass` or `verdict: fail` line and an `at: file line N` line
    VerdictIncomplete,
    /// Reads: `{key}:` needs the review and what it judges above it
    KeyWithoutReview { key: String },
    /// Reads: `verdict:` must be `pass` or `fail`
    VerdictNotPassOrFail,
    /// Reads: `at:` must be a file and a line, as `qc/looked_at.csv line 2`
    AtNotFileAndLine,
    /// Reads: expected source artifact: product\[dimension=value,...\]
    ExpectedSourceArtifact,
    /// Reads: a source record names no file; its source's path rule gives it, so remove `{path}`
    SourceRecordNamesNoFile { path: String },
    /// Reads: expected at least one source product
    ExpectedSourceProducts,
    /// Reads: expected nested group: \[dimension=value,...\]:
    ExpectedNestedGroup,
    /// Reads: nested group repeats dimension `{dimension}`
    NestedGroupRepeatsDimension { dimension: String },
    /// Reads: duplicate nested value `{value}`
    DuplicateNestedValue { value: String },
    /// Reads: nested group needs a value
    NestedGroupWithoutValue,
    /// Reads: expected context: \[dimension=value,...\]
    ExpectedContext,
    /// Reads: expected closing `\]` in source artifact
    UnclosedSourceArtifact,
    /// Reads: expected `dimension=value` in source artifact
    ExpectedBindingInArtifact,
    /// Reads: source dimension value must be one nonempty token
    ArtifactValueNotOneToken,
    /// Reads: duplicate source dimension `{dimension}`
    DuplicateSourceDimension { dimension: String },
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootAfterSection { word } => write!(f, "`{word}` comes before every section of a .spitout"),
            Self::SpitoutNamesOnce { what } => write!(f, "a .spitout names its {what} once"),
            Self::NamedContextsReplaced { name } => write!(f, "named context section `{name}` is replaced by `contexts:`"),
            Self::ExpectedSourcePath => f.write_str("expected source path: product: template"),
            Self::DuplicateSourcePathRule { name } => write!(f, "duplicate source path rule for `{name}`"),
            Self::SourceListWithoutContext => f.write_str("source list needs a parent context"),
            Self::SourceListNotIndented => f.write_str("source list must be indented under its context"),
            Self::ExpectedSectionHeader => f.write_str("expected inventory section header: sources:, contexts:, or contexts name:"),
            Self::KeyWithoutRemovedRecord { key } => write!(f, "`{key}:` needs the removed artifact or group above it"),
            Self::FoundNotInteger => f.write_str("`found:` must be a nonnegative integer"),
            Self::VerdictIncomplete => f.write_str("a verdict needs a `verdict: pass` or `verdict: fail` line and an `at: file line N` line"),
            Self::KeyWithoutReview { key } => write!(f, "`{key}:` needs the review and what it judges above it"),
            Self::VerdictNotPassOrFail => f.write_str("`verdict:` must be `pass` or `fail`"),
            Self::AtNotFileAndLine => f.write_str("`at:` must be a file and a line, as `qc/looked_at.csv line 2`"),
            Self::ExpectedSourceArtifact => f.write_str("expected source artifact: product[dimension=value,...]"),
            Self::SourceRecordNamesNoFile { path } => write!(f, "a source record names no file; its source's path rule gives it, so remove `{path}`"),
            Self::ExpectedSourceProducts => f.write_str("expected at least one source product"),
            Self::ExpectedNestedGroup => f.write_str("expected nested group: [dimension=value,...]:"),
            Self::NestedGroupRepeatsDimension { dimension } => write!(f, "nested group repeats dimension `{dimension}`"),
            Self::DuplicateNestedValue { value } => write!(f, "duplicate nested value `{value}`"),
            Self::NestedGroupWithoutValue => f.write_str("nested group needs a value"),
            Self::ExpectedContext => f.write_str("expected context: [dimension=value,...]"),
            Self::UnclosedSourceArtifact => f.write_str("expected closing `]` in source artifact"),
            Self::ExpectedBindingInArtifact => f.write_str("expected `dimension=value` in source artifact"),
            Self::ArtifactValueNotOneToken => f.write_str("source dimension value must be one nonempty token"),
            Self::DuplicateSourceDimension { dimension } => write!(f, "duplicate source dimension `{dimension}`"),
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
            InventoryError::RootAfterSection {
                word: "word".to_owned(),
            },
            InventoryError::SpitoutNamesOnce {
                what: "what".to_owned(),
            },
            InventoryError::NamedContextsReplaced {
                name: "name".to_owned(),
            },
            InventoryError::ExpectedSourcePath,
            InventoryError::DuplicateSourcePathRule {
                name: "name".to_owned(),
            },
            InventoryError::SourceListWithoutContext,
            InventoryError::SourceListNotIndented,
            InventoryError::ExpectedSectionHeader,
            InventoryError::KeyWithoutRemovedRecord {
                key: "key".to_owned(),
            },
            InventoryError::FoundNotInteger,
            InventoryError::VerdictIncomplete,
            InventoryError::KeyWithoutReview {
                key: "key".to_owned(),
            },
            InventoryError::VerdictNotPassOrFail,
            InventoryError::AtNotFileAndLine,
            InventoryError::ExpectedSourceArtifact,
            InventoryError::SourceRecordNamesNoFile {
                path: "path".to_owned(),
            },
            InventoryError::ExpectedSourceProducts,
            InventoryError::ExpectedNestedGroup,
            InventoryError::NestedGroupRepeatsDimension {
                dimension: "dimension".to_owned(),
            },
            InventoryError::DuplicateNestedValue {
                value: "value".to_owned(),
            },
            InventoryError::NestedGroupWithoutValue,
            InventoryError::ExpectedContext,
            InventoryError::UnclosedSourceArtifact,
            InventoryError::ExpectedBindingInArtifact,
            InventoryError::ArtifactValueNotOneToken,
            InventoryError::DuplicateSourceDimension {
                dimension: "dimension".to_owned(),
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
