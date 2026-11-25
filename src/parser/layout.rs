use std::borrow::Cow;
use std::ops::Range;

use super::keyword::{has_top_level_equals, Header, Keyword};
use crate::span::strip_comment;

#[derive(Clone, Debug, Default)]
pub(crate) struct Layout<'t> {
    pub(crate) lines: Vec<Line<'t>>,
    pub(crate) statements: Vec<Statement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Statement {
    pub(crate) lines: Range<usize>,
    pub(crate) parent: Option<usize>,
    pub(crate) opens: Option<Block>,
    pub(crate) blank: bool,
    pub(crate) stray: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Block {
    Stage,
    Body,
    Defaults,
    Keyword(Keyword),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Line<'t> {
    pub(crate) text: &'t str,
    pub(crate) indent: usize,
    pub(crate) uncommented: &'t str,
    pub(crate) code: &'t str,
    pub(crate) head: Head<'t>,
    pub(crate) record: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Head<'t> {
    Blank,
    Keyword(Keyword, &'t str),
    Header(Header<'t>),
    Step,
    Other,
}

impl<'t> Layout<'t> {
    pub(crate) fn new(text: &'t str) -> Self {
        let mut lines: Vec<_> = text.lines().map(Line::new).collect();
        let mut records = false;
        for line in &mut lines {
            match line.head {
                Head::Header(_) => records = true,
                Head::Keyword(..) | Head::Step => records = false,
                _ => {}
            }
            line.record = records;
        }
        let mut statements = Vec::with_capacity(lines.len());
        let mut open: Vec<(usize, usize, Block)> = Vec::new();
        let mut start = 0;
        while start < lines.len() {
            let first = &lines[start];
            if first.blank() {
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
            let keep = open
                .iter()
                .take_while(|&&(_, indent, _)| indent < first.indent)
                .count();
            let block = keep.checked_sub(1).map(|last| open[last].2);
            let entry = matches!(block, Some(Block::Keyword(_) | Block::Defaults));
            let stray = !entry && first.head == Head::Other && !first.code.contains('=');
            if !stray {
                open.truncate(keep);
            }
            let block = open.last().map(|&(_, _, block)| block);
            let parent = open.last().map(|&(opener, _, _)| opener);
            let mut end = brackets_end(&lines, start);
            let opens = opens(&lines[start..end], block);
            let keyword = match block {
                Some(Block::Keyword(keyword)) => Some(keyword),
                _ => first.keyword(),
            };
            let command = matches!(
                keyword,
                Some(
                    Keyword::Command
                        | Keyword::Verify
                        | Keyword::Check
                        | Keyword::Screen
                        | Keyword::Path
                )
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

    pub(crate) fn first(&self, statement: &Statement) -> &Line<'t> {
        &self.lines[statement.lines.start]
    }

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
            if !line.record {
                joined.push_str(line.uncommented);
            }
        }
        Cow::Owned(joined)
    }

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
            record: false,
        }
    }

    pub(crate) fn blank(&self) -> bool {
        self.head == Head::Blank || self.record
    }

    pub(crate) fn keyword(&self) -> Option<Keyword> {
        match self.head {
            Head::Keyword(keyword, _) => Some(keyword),
            _ => None,
        }
    }

    pub(crate) fn header(&self) -> Option<Header<'t>> {
        match self.head {
            Head::Header(header) => Some(header),
            _ => None,
        }
    }
}

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

fn opens(lines: &[Line], block: Option<Block>) -> Option<Block> {
    let first = &lines[0];
    let ends_in_colon = || {
        lines
            .iter()
            .rev()
            .find(|line| !line.blank())
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

fn brackets_end(lines: &[Line], start: usize) -> usize {
    let first = &lines[start];
    let mut depth = balance(first.uncommented);
    let mut end = start + 1;
    while depth > 0 {
        let Some(next) = lines.get(end) else {
            break;
        };
        if next.record {
            end += 1;
            continue;
        }
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

fn indented_end(lines: &[Line], start: usize, mut end: usize) -> usize {
    let indent = lines[start].indent;
    let mut next = end;
    while let Some(line) = lines.get(next) {
        if line.blank() {
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

    #[test]
    fn heads_follow_the_keyword_and_header_rules() {
        for line in ["contexts", "products:"] {
            assert_eq!(Layout::new(line).lines[0].head, Head::Other, "{line}");
        }
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
    fn records_are_marked_and_blank_to_statements() {
        let text = "x = f(a,\nsources:\n    raw[id=1]\n)\nsource raw : T [id]\n";
        let layout = Layout::new(text);
        let records: Vec<_> = layout.lines.iter().map(|line| line.record).collect();
        assert_eq!(records, [false, true, true, true, false]);
        let statements: Vec<_> = layout
            .statements
            .iter()
            .filter(|statement| !statement.blank)
            .map(|statement| statement.lines.clone())
            .collect();
        assert_eq!(statements, [0..4, 4..5]);
        assert_eq!(layout.text(&layout.statements[0]), "x = f(a,\n\n\n");
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
        assert_eq!(layout.text(&layout.statements[2]), "operation g(");
    }

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
                (8, Some(6), None),
                (9, Some(6), None),
                (10, Some(1), None),
                (11, None, None),
                (12, None, None),
                (13, None, Some(Stage)),
                (14, Some(13), None),
                (15, Some(13), None),
                (16, Some(13), None),
            ]
        );
    }

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
    a @where(id=1)
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
                (16..17, None, None),
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
