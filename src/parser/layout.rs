//! A document's lines, each read once: its indentation, its code without
//! its comment, and what it starts with; the statements those lines make;
//! and the blocks those statements open. Every reader of a document's lines
//! takes them from here, so they agree on what each line is.
//!
//! Structure comes from the lines' surface alone, before any statement's
//! content is parsed: which lines make a statement, which statements open a
//! block, and which block holds each. A statement whose content fails to
//! parse still has its place in that structure, so a failure never changes
//! how the lines after it are read.

use std::borrow::Cow;
use std::ops::Range;

use super::keyword::{Header, Keyword};
use super::lexical::strip_comment;

/// The lines of one text, as `str::lines` gives them, and its statements.
#[derive(Clone, Debug, Default)]
pub(crate) struct Layout<'t> {
    pub(crate) lines: Vec<Line<'t>>,
    /// Each statement, in order. Every line is in exactly one.
    pub(crate) statements: Vec<Statement>,
}

/// One statement: a line, or an `operation` signature whose parentheses
/// stay open past its first line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Statement {
    /// The indices of the lines it spans.
    pub(crate) lines: Range<usize>,
    /// The statement whose block holds it, by its index: the nearest
    /// statement before it that opens a block and is indented less. `None`
    /// at the top level, and for a blank statement.
    pub(crate) parent: Option<usize>,
    /// The block it opens, if it opens one.
    pub(crate) opens: Option<Block>,
    /// It holds no code.
    pub(crate) blank: bool,
    /// It can start no statement: it has no keyword and no `=`, so it
    /// fails wherever it stands. It neither opens nor closes a block, so
    /// that a stray line, such as a typo at the start of a line, does not
    /// move the lines after it out of their block.
    pub(crate) stray: bool,
}

/// A block of lines indented beneath the statement that opens it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Block {
    /// A `stage` line's: the stage's lines, whatever the line holds.
    Stage,
    /// An `operation` header's that ends in `:`: the steps that carry the
    /// operation out. Nothing in a body opens a block.
    Body,
    /// A `default:` line's: the `path:`, `ext:` and `check:` defaults of
    /// the file or stage it is in. Nothing in it opens a block.
    Defaults,
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
        let lines: Vec<_> = text.lines().map(Line::new).collect();
        let mut statements = Vec::with_capacity(lines.len());
        // The blocks open at the statement being read, innermost last: the
        // statement that opens each, its indentation, and its kind.
        let mut open: Vec<(usize, usize, Block)> = Vec::new();
        let mut start = 0;
        while start < lines.len() {
            let range = start..signature_end(&lines, start);
            start = range.end;
            let first = &lines[range.start];
            let blank = range.len() == 1 && first.head == Head::Blank;
            if blank {
                statements.push(Statement {
                    lines: range,
                    parent: None,
                    opens: None,
                    blank,
                    stray: false,
                });
                continue;
            }
            let stray = first.head == Head::Other && !first.code.contains('=');
            while !stray
                && open
                    .last()
                    .is_some_and(|&(_, indent, _)| first.indent <= indent)
            {
                open.pop();
            }
            let parent = open.last().map(|&(opener, _, _)| opener);
            // Only a stage's lines may open blocks.
            let in_body = open
                .last()
                .is_some_and(|&(_, _, block)| block != Block::Stage);
            let opens = (!in_body).then(|| opens(&lines[range.clone()])).flatten();
            if let Some(block) = opens {
                open.push((statements.len(), first.indent, block));
            }
            statements.push(Statement {
                lines: range,
                parent,
                opens,
                blank,
                stray,
            });
        }
        Self { lines, statements }
    }

    /// The first line of `statement`.
    pub(crate) fn first(&self, statement: &Statement) -> &Line<'t> {
        &self.lines[statement.lines.start]
    }

    /// The text of `statement`. A statement of one line is the line as
    /// written. One of several is each line without its comment, joined by
    /// newlines, so that a token's address within it still gives its line
    /// and column (`ParseError::locate`).
    pub(crate) fn text(&self, statement: &Statement) -> Cow<'t, str> {
        let lines = statement.lines.clone();
        if lines.len() == 1 {
            return Cow::Borrowed(self.lines[lines.start].text);
        }
        let mut joined = String::new();
        for (index, line) in self.lines[lines].iter().enumerate() {
            if index > 0 {
                joined.push('\n');
            }
            joined.push_str(line.uncommented);
        }
        Cow::Owned(joined)
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

/// The block a statement on `lines` opens: a `stage` line's, whatever it
/// holds, an `operation` header's that ends in `:`, and `default:`'s.
fn opens(lines: &[Line]) -> Option<Block> {
    match lines[0].keyword()? {
        Keyword::Stage => Some(Block::Stage),
        Keyword::Default if lines[0].code == "default:" => Some(Block::Defaults),
        Keyword::Operation => lines
            .iter()
            .rev()
            .find(|line| !line.code.is_empty())
            .filter(|line| line.code.ends_with(':'))
            .map(|_| Block::Body),
        _ => None,
    }
}

/// One past the last line of the statement that starts at `start`: an
/// `operation` signature takes the lines after it while its parentheses are
/// open, and any other line is a statement of its own.
fn signature_end(lines: &[Line], start: usize) -> usize {
    let first = &lines[start];
    if first.keyword() != Some(Keyword::Operation) {
        return start + 1;
    }
    let mut depth = balance(first.uncommented);
    let mut end = start + 1;
    while depth > 0 {
        let Some(next) = lines.get(end) else {
            break;
        };
        // A declaration or a body step is never part of a signature, even
        // when a missing closer leaves the parentheses open.
        if !next.code.is_empty()
            && (next.keyword().is_some()
                || next.code.contains('=')
                || (next.indent <= first.indent && !next.code.starts_with(')')))
        {
            break;
        }
        depth += balance(next.uncommented);
        end += 1;
    }
    end
}

fn balance(text: &str) -> isize {
    text.bytes().fold(0, |depth, byte| match byte {
        b'(' => depth + 1,
        b')' => depth - 1,
        _ => depth,
    })
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

    #[test]
    fn an_operation_signature_spans_lines_while_its_parentheses_are_open() {
        let text = "operation f(\n    x: A,  # first\n\n    y: B,\n) -> C\nz = f(a, b)\noperation g(\nw = g(z)\n";
        let layout = Layout::new(text);
        let lines: Vec<_> = layout.statements.iter().map(|s| s.lines.clone()).collect();
        assert_eq!(lines, [0..5, 5..6, 6..7, 7..8]);
        assert_eq!(
            layout.text(&layout.statements[0]),
            "operation f(\n    x: A,  \n\n    y: B,\n) -> C"
        );
        // An unclosed signature stops before a step, which is never part of one.
        assert_eq!(layout.text(&layout.statements[2]), "operation g(");
    }

    /// Each statement's parent and the block it opens, from indentation and
    /// the statement's first word alone.
    #[test]
    fn blocks_come_from_indentation_and_the_opening_line() {
        let text = "\
source a : A [s]
stage pre        # no colon, but a stage line opens a block
    b = f(a)

    stage inner:
        c = f(b)
    operation g(
        x: A,
    ) -> B:
        y = f(x)
        stage nested:
            z = f(y)
    d = f(c)
operation h(x: A) -> B
    e = f(a)
stage post:
    g = f(a)
frob 1
    k = f(g)
";
        let layout = Layout::new(text);
        let shape: Vec<_> = layout
            .statements
            .iter()
            .enumerate()
            .filter(|(_, statement)| !statement.blank)
            .map(|(index, statement)| (index, statement.parent, statement.opens))
            .collect();
        use super::Block::{Body, Stage};
        assert_eq!(
            shape,
            [
                (0, None, None),
                (1, None, Some(Stage)),
                (2, Some(1), None),
                (4, Some(1), Some(Stage)),
                (5, Some(4), None),
                (6, Some(1), Some(Body)),
                (7, Some(6), None),
                // Nothing in a body opens a block.
                (8, Some(6), None),
                (9, Some(6), None),
                (10, Some(1), None),
                (11, None, None),
                // An operation without a body opens none, so a line indented
                // beneath it is at the top level.
                (12, None, None),
                (13, None, Some(Stage)),
                (14, Some(13), None),
                // A line that can start no statement closes no block.
                (15, Some(13), None),
                (16, Some(13), None),
            ]
        );
    }
}
