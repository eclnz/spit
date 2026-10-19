//! A document's lines, each read once: its indentation, its code without
//! its comment, and what it starts with. Every reader of a document's lines
//! takes them from here, so they agree on what each line is.

use super::keyword::{Header, Keyword};
use super::lexical::strip_comment;

/// The lines of one text, as `str::lines` gives them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Layout<'t> {
    pub(crate) lines: Vec<Line<'t>>,
}

/// One physical line.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line<'t> {
    /// The line as written.
    pub(crate) text: &'t str,
    /// The bytes before its first non-space character.
    pub(crate) indent: usize,
    /// The line without its comment, untrimmed, so that a byte offset in it
    /// is one in `text`.
    pub(crate) uncommented: &'t str,
    /// The line without its comment, trimmed.
    pub(crate) code: &'t str,
    pub(crate) head: Head<'t>,
}

/// What a line's code starts with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Head<'t> {
    /// No code: an empty line, or one that is only a comment.
    Blank,
    /// A statement's keyword, and the code after it.
    Keyword(Keyword, &'t str),
    /// A records header such as `sources:`.
    Header(Header<'t>),
    /// Anything else: a step, a record, or a line that fails to parse.
    Other,
}

impl<'t> Layout<'t> {
    pub(crate) fn new(text: &'t str) -> Self {
        Self {
            lines: text.lines().map(Line::new).collect(),
        }
    }

    /// Each line with its number, from 1.
    pub(crate) fn numbered(&self) -> impl Iterator<Item = (usize, &Line<'t>)> {
        self.lines
            .iter()
            .enumerate()
            .map(|(index, line)| (index + 1, line))
    }
}

impl<'t> Line<'t> {
    pub(crate) fn new(text: &'t str) -> Self {
        let uncommented = strip_comment(text);
        let code = uncommented.trim();
        // Header and keyword lines never overlap: a header has no space
        // before its `:` but in `contexts name:`, and `contexts` is no
        // keyword.
        let head = if code.is_empty() {
            Head::Blank
        } else if let Some(header) = Header::of(code) {
            Head::Header(header)
        } else if let Some((keyword, rest)) = Keyword::split(code) {
            Head::Keyword(keyword, rest)
        } else {
            Head::Other
        };
        Self {
            text,
            indent: text.len() - text.trim_start().len(),
            uncommented,
            code,
            head,
        }
    }

    /// The keyword the line starts with, if one.
    pub(crate) fn keyword(&self) -> Option<Keyword> {
        match self.head {
            Head::Keyword(keyword, _) => Some(keyword),
            _ => None,
        }
    }

    /// The records header the line is, if one.
    pub(crate) fn header(&self) -> Option<Header<'t>> {
        match self.head {
            Head::Header(header) => Some(header),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Head, Layout};
    use crate::parser::{Header, Keyword};

    #[test]
    fn each_line_is_read_once_with_its_head() {
        let layout = Layout::new(
            "use lib.spit  # library\n\n  # note\nsources:\n    image[sub=1]\nx = f(y)\r\n",
        );
        let heads: Vec<_> = layout.lines.iter().map(|line| line.head).collect();
        assert_eq!(
            heads,
            [
                Head::Keyword(Keyword::Use, "lib.spit"),
                Head::Blank,
                Head::Blank,
                Head::Header(Header::Sources),
                Head::Other,
                Head::Other,
            ]
        );
        let record = &layout.lines[4];
        assert_eq!((record.indent, record.code), (4, "image[sub=1]"));
        assert_eq!(layout.lines[0].uncommented, "use lib.spit  ");
        assert_eq!(layout.lines[5].text, "x = f(y)");
        assert_eq!(layout.numbered().nth(3).map(|(number, _)| number), Some(4));
    }

    /// The keyword and header tests in `keyword.rs` decide every head, so a
    /// line no test there covers reads the same here.
    #[test]
    fn heads_follow_the_keyword_and_header_rules() {
        for line in [
            "stage = f(x)",
            "check : Report = f(x)",
            "contexts",
            "products:",
        ] {
            assert_eq!(Layout::new(line).lines[0].head, Head::Other, "{line}");
        }
        assert_eq!(
            Layout::new("contexts visits:").lines[0].header(),
            Some(Header::Contexts(Some("visits")))
        );
        assert_eq!(
            Layout::new("path: out/{@entities}").lines[0].keyword(),
            Some(Keyword::Path)
        );
    }
}
