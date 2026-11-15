//! Errors the parser reads from `entities`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `entities`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityError {
    /// Says: expected `default entities: {@key}={@value} separated  "__ "`
    ExpectedDefaultEntitiesSeparated,
    /// Says: unexpected text after the entities separator; a product with no dimensions has no `{@entities}`, so put it in `\[...\]`
    UnexpectedTextAfterEntitiesSeparatorProduct,
    /// Says: expected `default entities dimension: label`
    ExpectedDefaultEntitiesDimensionLabel,
    /// Says: the entities separator must be quoted
    EntitiesSeparatorMustQuoted,
    /// Says: unclosed quoted entities text
    UnclosedQuotedEntitiesText,
    /// Says: expected whitespace after quoted entities text
    ExpectedWhitespaceAfterQuotedEntitiesText,
    /// Says: {message}
    Format { message: String },
}

impl fmt::Display for EntityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedDefaultEntitiesSeparated => f.write_str("expected `default entities: {@key}={@value} separated \"__\"`"),
            Self::UnexpectedTextAfterEntitiesSeparatorProduct => f.write_str("unexpected text after the entities separator; a product with no dimensions has no `{@entities}`, so put it in `[...]`"),
            Self::ExpectedDefaultEntitiesDimensionLabel => f.write_str("expected `default entities dimension: label`"),
            Self::EntitiesSeparatorMustQuoted => f.write_str("the entities separator must be quoted"),
            Self::UnclosedQuotedEntitiesText => f.write_str("unclosed quoted entities text"),
            Self::ExpectedWhitespaceAfterQuotedEntitiesText => f.write_str("expected whitespace after quoted entities text"),
            Self::Format { message } => write!(f, "{message}"),
        }
    }
}
