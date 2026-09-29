//! What a line starts with: a keyword such as `source` or `path`, or a
//! section header such as `products:` or `contexts name:`. Every reader of
//! a document classifies its lines here, so they agree on what each is.

/// A keyword that starts a statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Keyword {
    Use,
    Source,
    Discover,
    Operation,
    Command,
    Verify,
    Require,
    Skip,
    /// `path:` for a default, `path product:` for one product.
    Path,
    Stage,
    /// The removed `shell-source:` line, kept to explain its removal.
    ShellSource,
}

const WORDS: [(Keyword, &str); 9] = [
    (Keyword::Use, "use"),
    (Keyword::Source, "source"),
    (Keyword::Discover, "discover"),
    (Keyword::Operation, "operation"),
    (Keyword::Command, "command"),
    (Keyword::Verify, "verify"),
    (Keyword::Require, "require"),
    (Keyword::Skip, "skip"),
    (Keyword::Stage, "stage"),
];

impl Keyword {
    /// The keyword `line` starts with, and the text after it. A trimmed
    /// line is expected. `stage` opens a stage only without `=`, since a
    /// step's output product may be called `stage`.
    pub(crate) fn split(line: &str) -> Option<(Self, &str)> {
        if let Some(rest) = line.strip_prefix("shell-source:") {
            return Some((Self::ShellSource, rest));
        }
        if let Some(rest) = line.strip_prefix("path") {
            if rest.starts_with([' ', ':']) {
                return Some((Self::Path, rest));
            }
        }
        let (word, rest) = line.split_once(' ')?;
        let (keyword, _) = WORDS.iter().find(|(_, name)| *name == word)?;
        if *keyword == Self::Stage && line.contains('=') {
            return None;
        }
        Some((*keyword, rest))
    }

    pub(crate) fn of(line: &str) -> Option<Self> {
        Self::split(line).map(|(keyword, _)| keyword)
    }
}

/// A line that opens a section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Header<'a> {
    Products,
    Operations,
    Pipeline,
    Constraints,
    Commands,
    Sources,
    /// `contexts:`, or `contexts name:` for a named discovery's contexts.
    Contexts(Option<&'a str>),
}

impl<'a> Header<'a> {
    /// The header a trimmed `line` is, if it is one.
    pub(crate) fn of(line: &'a str) -> Option<Self> {
        Some(match line {
            "products:" => Self::Products,
            "operations:" => Self::Operations,
            "pipeline:" => Self::Pipeline,
            "constraints:" => Self::Constraints,
            "commands:" => Self::Commands,
            "sources:" => Self::Sources,
            "contexts:" => Self::Contexts(None),
            _ => Self::Contexts(Some(
                line.strip_prefix("contexts ")?.strip_suffix(':')?.trim(),
            )),
        })
    }

    /// Whether it opens records, which belong in a `.spitout`, rather than a
    /// section of a sectioned pipeline.
    pub(crate) fn is_records(self) -> bool {
        matches!(self, Self::Sources | Self::Contexts(_))
    }
}

#[cfg(test)]
mod tests {
    use super::{Header, Keyword};

    #[test]
    fn lines_are_classified_by_their_first_word() {
        assert_eq!(
            Keyword::split("source raw [id]"),
            Some((Keyword::Source, "raw [id]"))
        );
        assert_eq!(Keyword::of("path: out/{entities}"), Some(Keyword::Path));
        assert_eq!(Keyword::of("path raw: in/{id}"), Some(Keyword::Path));
        assert_eq!(Keyword::of("paths = f(x)"), None);
        assert_eq!(Keyword::of("stage analysis:"), Some(Keyword::Stage));
        assert_eq!(Keyword::of("stage = f(x)"), None);
        assert_eq!(Keyword::of("sources:"), None);
        assert_eq!(
            Header::of("contexts visits:"),
            Some(Header::Contexts(Some("visits")))
        );
        assert_eq!(Header::of("products:"), Some(Header::Products));
        assert_eq!(Header::of("contexts"), None);
        assert!(Header::of("sources:").is_some_and(Header::is_records));
    }
}
