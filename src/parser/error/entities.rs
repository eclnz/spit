//! The errors of `default entities:` spellings; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `entities`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityError {
    /// Reads: expected `default entities: {@key}={@value} separated "__"`
    ExpectedEntitiesFormat,
    /// Reads: unexpected text after the entities separator; a product with no dimensions has no `{@entities}`, so put it in `\[...\]`
    TextAfterEntitiesSeparator,
    /// Reads: expected `default entities dimension: label`
    ExpectedEntityLabel,
    /// Reads: the entities separator must be quoted
    SeparatorNotQuoted,
    /// Reads: unclosed quoted entities text
    UnclosedQuotedEntitiesText,
    /// Reads: expected whitespace after quoted entities text
    ExpectedSpaceAfterQuote,
    /// A `default entities:` spelling that does not parse, as `EntitiesFormat` words it.
    Format { message: String },
}

impl fmt::Display for EntityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedEntitiesFormat => f.write_str("expected `default entities: {@key}={@value} separated \"__\"`"),
            Self::TextAfterEntitiesSeparator => f.write_str("unexpected text after the entities separator; a product with no dimensions has no `{@entities}`, so put it in `[...]`"),
            Self::ExpectedEntityLabel => f.write_str("expected `default entities dimension: label`"),
            Self::SeparatorNotQuoted => f.write_str("the entities separator must be quoted"),
            Self::UnclosedQuotedEntitiesText => f.write_str("unclosed quoted entities text"),
            Self::ExpectedSpaceAfterQuote => f.write_str("expected whitespace after quoted entities text"),
            Self::Format { message } => write!(f, "{message}"),
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
            EntityError::ExpectedEntitiesFormat,
            EntityError::TextAfterEntitiesSeparator,
            EntityError::ExpectedEntityLabel,
            EntityError::SeparatorNotQuoted,
            EntityError::UnclosedQuotedEntitiesText,
            EntityError::ExpectedSpaceAfterQuote,
            EntityError::Format {
                message: "message".to_owned(),
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
