//! Errors the parser reads from `lexical`; [`fmt::Display`] words each.

use std::fmt;

/// The errors of `lexical`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LexicalError {
    /// Says: unexpected `{closer}`
    UnexpectedCloser { closer: String },
    /// Says: unclosed `{opener}`
    UnclosedBracket { opener: String },
    /// Says: empty item in comma-separated list
    EmptyListItem,
    /// Says: expected `(` in operation call
    ExpectedOpenParen,
    /// Says: expected closing `)`
    ExpectedCloseParen,
    /// Says: invalid {kind} `{name}`; use letters, digits, and underscores
    InvalidName { kind: String, name: String },
    /// Says: `{name}` is one of SPIT's keywords, so no product may take it as its name; rename it
    ReservedName { name: String },
    /// Says: `{text}` is not an extension; write one such as `.mat` or `.nii.gz`, with letters, digits, `-` or `_` after each `.`
    NotAnExtension { text: String },
    /// Says: a `+` suffix follows the whole name of the file it is written beside; add `beside` and that file's port or source, as in `-> Fai +.fai beside fasta`, or drop the `+`
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
