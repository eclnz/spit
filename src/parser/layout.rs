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

use super::keyword::{has_top_level_equals, Header, Keyword};
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
    /// A keyword's alone with its `:`, as `source:`: statements of that
    /// keyword, each written without it. Only an `operation:` block's
    /// entries open blocks, their bodies.
    Keyword(Keyword),
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
    /// A step, `outputs = operation(inputs)` (see [`is_step`]).
    Step,
    /// Anything else: a record, or a line that fails to parse.
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
            let first = &lines[start];
            if first.head == Head::Blank {
                statements.push(Statement {
                    lines: start..start + 1,
                    parent: None,
                    opens: None,
                    blank: true,
                    stray: false,
                });
                start += 1;
                continue;
            }
            // The blocks the line is indented beneath.
            let keep = open
                .iter()
                .take_while(|&&(_, indent, _)| indent < first.indent)
                .count();
            let block = keep.checked_sub(1).map(|last| open[last].2);
            // An entry of a keyword block has no keyword of its own, so it
            // is no stray line.
            let entry = matches!(block, Some(Block::Keyword(_) | Block::Defaults));
            let stray = !entry && first.head == Head::Other && !first.code.contains('=');
            if !stray {
                open.truncate(keep);
            }
            let block = open.last().map(|&(_, _, block)| block);
            let parent = open.last().map(|&(opener, _, _)| opener);
            let mut end = brackets_end(&lines, start);
            let opens = opens(&lines[start..end], block);
            // A command goes on over the lines indented beneath it.
            let keyword = match block {
                Some(Block::Keyword(keyword)) => Some(keyword),
                _ => first.keyword(),
            };
            let command = matches!(
                keyword,
                Some(Keyword::Command | Keyword::Verify | Keyword::Check)
            );
            if command && opens.is_none() {
                end = indented_end(&lines, start, end);
            }
            if let Some(block) = opens {
                open.push((statements.len(), first.indent, block));
            }
            statements.push(Statement {
                lines: start..end,
                parent,
                opens,
                blank: false,
                stray,
            });
            start = end;
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
        } else if is_step(code) {
            Head::Step
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

/// Whether `code` reads as a step, `outputs = operation(inputs)`: names,
/// with any types and dimensions, then `=`, a name and `(`. A command's
/// word such as `--threads=4` or `KEY=value` does not, nor does a record
/// such as `image[sub=1]`. This is the one rule for a step: every reader
/// of a document takes it from a line's [`Head`].
pub(crate) fn is_step(code: &str) -> bool {
    let Some((outputs, call)) = code.split_once('=') else {
        return false;
    };
    let name = |text: &str| {
        text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
    };
    name(
        outputs
            .trim()
            .split([',', ':', ' '])
            .next()
            .unwrap_or_default(),
    ) && outputs
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " \t_,:<>[]".contains(c))
        && call
            .split_once('(')
            .is_some_and(|(operation, _)| name(operation.trim()))
}

/// The block a statement on `lines`, in `block`, opens. Where a stage's
/// lines or the top level may open blocks: a `stage` line's, whatever it
/// holds; an `operation` header's that ends in `:`; `default:`'s; and a
/// keyword's alone with its `:`, as `source:`. In an `operation:` block, an
/// entry that ends in `:` opens its body. Nothing else opens a block.
fn opens(lines: &[Line], block: Option<Block>) -> Option<Block> {
    let first = &lines[0];
    let ends_in_colon = || {
        lines
            .iter()
            .rev()
            .find(|line| !line.code.is_empty())
            .is_some_and(|line| line.code.ends_with(':'))
    };
    match block {
        None | Some(Block::Stage) => {}
        Some(Block::Keyword(Keyword::Operation)) => {
            return ends_in_colon().then_some(Block::Body);
        }
        Some(_) => return None,
    }
    match first.head {
        Head::Keyword(keyword, ":") if lines.len() == 1 && keyword != Keyword::Default => {
            Some(Block::Keyword(keyword))
        }
        Head::Keyword(Keyword::Stage, _) => Some(Block::Stage),
        Head::Keyword(Keyword::Default, _) if first.code == "default:" => Some(Block::Defaults),
        Head::Keyword(Keyword::Operation, _) => ends_in_colon().then_some(Block::Body),
        _ => None,
    }
}

/// One past the last line of the statement that starts at `start`, going
/// on while its parentheses are open, as an `operation` signature or a
/// step's arguments may. A line that starts a declaration or a step, or is
/// indented no deeper than the first and does not close the parentheses,
/// is never part of it, even when a missing closer leaves them open.
fn brackets_end(lines: &[Line], start: usize) -> usize {
    let first = &lines[start];
    let mut depth = balance(first.uncommented);
    let mut end = start + 1;
    while depth > 0 {
        let Some(next) = lines.get(end) else {
            break;
        };
        if !next.code.is_empty()
            && (next.keyword().is_some()
                || has_top_level_equals(next.code)
                || (next.indent <= first.indent && !next.code.starts_with(')')))
        {
            break;
        }
        depth += balance(next.uncommented);
        end += 1;
    }
    end
}

/// One past the last line of a command that starts at `start` and has read
/// up to `end`: it takes each line after that is indented deeper than its
/// first, with the blank lines between them, up to one that starts a
/// statement of its own, with a keyword or as a step, so that a statement
/// indented by mistake is never read as a command's words.
fn indented_end(lines: &[Line], start: usize, mut end: usize) -> usize {
    let indent = lines[start].indent;
    let mut next = end;
    while let Some(line) = lines.get(next) {
        if line.head == Head::Blank {
            next += 1;
        } else if line.indent > indent && matches!(line.head, Head::Other) {
            next += 1;
            end = next;
        } else {
            break;
        }
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
                Head::Step,
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
        for line in ["contexts", "products:"] {
            assert_eq!(Layout::new(line).lines[0].head, Head::Other, "{line}");
        }
        // A keyword's word is reserved: a line starting with one is no step.
        for line in ["stage = f(x)", "check : Report = f(x)", "default = f(x)"] {
            assert!(Layout::new(line).lines[0].keyword().is_some(), "{line}");
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

    /// A keyword alone with its `:` opens a block of its entries; in an
    /// `operation:` block, an entry ending in `:` opens its body. A command
    /// goes on over the lines indented beneath it, and any statement while
    /// its brackets are open.
    #[test]
    fn keyword_blocks_and_statements_over_several_lines() {
        use super::Block::{Body, Keyword as Of};
        let text = "\
command:
    run: tool {a}
        --flag

        {@output}
    other: tool
operation:
    twice(x: A) -> (out: A):
        out = step(x)
    pair(
        x: A
    ) -> A
step = f(
    a @ where(id=1)
)
source raw : T [id]
    stray = f(raw)
command copy: cp --mode=fast
    {input}
    copied = copy(raw)
    operation late(x: A) -> A
";
        let layout = Layout::new(text);
        let shape: Vec<_> = layout
            .statements
            .iter()
            .filter(|statement| !statement.blank)
            .map(|statement| (statement.lines.clone(), statement.parent, statement.opens))
            .collect();
        assert_eq!(
            shape,
            [
                (0..1, None, Some(Of(Keyword::Command))),
                (1..5, Some(0), None),
                (5..6, Some(0), None),
                (6..7, None, Some(Of(Keyword::Operation))),
                (7..8, Some(3), Some(Body)),
                (8..9, Some(4), None),
                (9..12, Some(3), None),
                (12..15, None, None),
                (15..16, None, None),
                // Only a command goes on over an indented line.
                (16..17, None, None),
                // And not over one that starts a statement of its own.
                (17..19, None, None),
                (19..20, None, None),
                (20..21, None, None),
            ]
        );
    }

    #[test]
    fn a_step_is_names_then_a_call() {
        for step in [
            "a = f(x)",
            "a, b = f(x)",
            "a : T [id] = f(x)",
            "m : Map<A,B> = ns::f(x)",
        ] {
            assert_eq!(Layout::new(step).lines[0].head, Head::Step, "{step}");
        }
        for word in [
            "--threads=4",
            "KEY=value tool",
            "x=1",
            "a = b",
            "{@output}=f(x)",
            "image[sub=1]",
        ] {
            assert_eq!(Layout::new(word).lines[0].head, Head::Other, "{word}");
        }
    }
}
