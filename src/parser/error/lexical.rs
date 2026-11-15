//! The errors of names, extensions and comma-separated lists; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `lexical`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LexicalError {
    /// Reads: unexpected `{closer}`
    UnexpectedCloser { closer: char },
    /// Reads: unclosed `{opener}`
    UnclosedBracket { opener: char },
    /// Reads: empty item in comma-separated list
    EmptyListItem,
    /// Reads: expected `(` in operation call
    ExpectedOpenParen,
    /// Reads: expected closing `)`
    ExpectedCloseParen,
    /// Reads: invalid {kind} `{name}`; use letters, digits, and underscores
    InvalidName { kind: &'static str, name: String },
    /// Reads: `{name}` is one of SPIT's keywords, so no product may take it as its name; rename it
    ReservedName { name: String },
    /// Reads: `{text}` is not an extension; write one such as `.mat` or `.nii.gz`, with letters, digits, `-` or `_` after each `.`
    NotAnExtension { text: String },
    /// Reads: a `+` suffix follows the whole name of the file it is written beside; add `beside` and that file's port or source, as in `-> Fai +.fai beside fasta`, or drop the `+`
    PlusSuffixWithoutBeside,
}

impl fmt::Display for LexicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedCloser { closer } => write!(f, "unexpected `{closer}`"),
            Self::UnclosedBracket { opener } => write!(f, "unclosed `{opener}`"),
            Self::EmptyListItem => f.write_str("empty item in comma-separated list"),
            Self::ExpectedOpenParen => f.write_str("expected `(` in operation call"),
            Self::ExpectedCloseParen => f.write_str("expected closing `)`"),
            Self::InvalidName { kind, name } => write!(f, "invalid {kind} `{name}`; use letters, digits, and underscores"),
            Self::ReservedName { name } => write!(f, "`{name}` is one of SPIT's keywords, so no product may take it as its name; rename it"),
            Self::NotAnExtension { text } => write!(f, "`{text}` is not an extension; write one such as `.mat` or `.nii.gz`, with letters, digits, `-` or `_` after each `.`"),
            Self::PlusSuffixWithoutBeside => f.write_str("a `+` suffix follows the whole name of the file it is written beside; add `beside` and that file's port or source, as in `-> Fai +.fai beside fasta`, or drop the `+`"),
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
            LexicalError::UnexpectedCloser { closer: '(' },
            LexicalError::UnclosedBracket { opener: '(' },
            LexicalError::EmptyListItem,
            LexicalError::ExpectedOpenParen,
            LexicalError::ExpectedCloseParen,
            LexicalError::InvalidName {
                kind: "kind",
                name: "name".to_owned(),
            },
            LexicalError::ReservedName {
                name: "name".to_owned(),
            },
            LexicalError::NotAnExtension {
                text: "text".to_owned(),
            },
            LexicalError::PlusSuffixWithoutBeside,
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
