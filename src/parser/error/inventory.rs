use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InventoryError {
    RootAfterSection { word: String },
    SpitoutNamesOnce { what: String },
    NamedContextsReplaced { name: String },
    ExpectedSourcePath,
    DuplicateSourcePathRule { name: String },
    SourceListWithoutContext,
    SourceListNotIndented,
    ExpectedSectionHeader,
    KeyWithoutRemovedRecord { key: String },
    FoundNotInteger,
    ExpectedReviewFile,
    ExpectedSourceArtifact,
    TextAfterRecord { path: String },
    ExpectedSourceProducts,
    ExpectedNestedGroup,
    NestedGroupRepeatsDimension { dimension: String },
    DuplicateNestedValue { value: String },
    NestedGroupWithoutValue,
    ExpectedContext,
    UnclosedSourceArtifact,
    ExpectedBindingInArtifact,
    ArtifactValueNotOneToken,
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
            Self::ExpectedReviewFile => f.write_str("expected a review and its file, as `looked_at from qc/looked_at.csv`"),
            Self::ExpectedSourceArtifact => f.write_str("expected source artifact: product[dimension=value,...]"),
            Self::TextAfterRecord { path } => write!(f, "`{path}` follows a source record; a record names its file after `: `, as `t1[id=4]: raw/4/T1`"),
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
            InventoryError::ExpectedReviewFile,
            InventoryError::ExpectedSourceArtifact,
            InventoryError::TextAfterRecord {
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
        crate::parser::error::assert_worded(variants);
    }
}
