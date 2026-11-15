//! One reader of bracket nesting for every parser that looks for something
//! "outside the brackets": a top-level comma, `@` or `=`, or the `)` that
//! closes a `(`. They share this scanner, so they agree on what is nested.

use std::str::CharIndices;

/// A kind of bracket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Paren,
    Angle,
    Square,
}

impl Kind {
    fn opener(self) -> char {
        match self {
            Self::Paren => '(',
            Self::Angle => '<',
            Self::Square => '[',
        }
    }

    fn closer(self) -> char {
        match self {
            Self::Paren => ')',
            Self::Angle => '>',
            Self::Square => ']',
        }
    }

    fn slot(self) -> usize {
        self as usize
    }
}

/// Every kind of bracket, in the order [`Scanner::unclosed`] reports them.
pub(super) const ALL: &[Kind] = &[Kind::Paren, Kind::Angle, Kind::Square];

/// Parentheses alone.
pub(super) const PARENS: &[Kind] = &[Kind::Paren];

/// One character of the scanned text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Mark {
    /// The byte index of the character.
    pub(super) index: usize,
    pub(super) character: char,
    /// How many brackets are open around the character. A bracket itself
    /// has the depth of the text around it, so `(` and its `)` agree.
    pub(super) depth: usize,
    /// A closing bracket that has no opener of its kind.
    pub(super) stray: bool,
}

/// The characters of a text with the depth of each, counting only the
/// brackets of `kinds`. A closer matches an opener of its own kind, so `(]`
/// is a stray `]` and an unclosed `(`.
///
/// Read the whole text, then call [`Scanner::unclosed`] for what was left open.
pub(super) struct Scanner<'a> {
    characters: CharIndices<'a>,
    kinds: &'static [Kind],
    /// The byte index of each opener not yet closed, for each kind by [`Kind::slot`].
    open: [Vec<usize>; 3],
}

impl<'a> Scanner<'a> {
    pub(super) fn new(text: &'a str, kinds: &'static [Kind]) -> Self {
        Self {
            characters: text.char_indices(),
            kinds,
            open: [Vec::new(), Vec::new(), Vec::new()],
        }
    }

    fn depth(&self) -> usize {
        self.open.iter().map(Vec::len).sum()
    }

    /// The earliest opener still open once the text is read, with its index.
    /// A parenthesis comes before an angle, and an angle before a square
    /// bracket, so the same text always gives the same one.
    pub(super) fn unclosed(&self) -> Option<(char, usize)> {
        ALL.iter().find_map(|kind| {
            let &index = self.open[kind.slot()].first()?;
            Some((kind.opener(), index))
        })
    }
}

impl Iterator for Scanner<'_> {
    type Item = Mark;

    fn next(&mut self) -> Option<Mark> {
        let (index, character) = self.characters.next()?;
        let mut stray = false;
        let mut depth = self.depth();
        if let Some(kind) = self.kinds.iter().find(|kind| kind.opener() == character) {
            self.open[kind.slot()].push(index);
        } else if let Some(kind) = self.kinds.iter().find(|kind| kind.closer() == character) {
            if self.open[kind.slot()].pop().is_some() {
                depth -= 1;
            } else {
                stray = true;
            }
        }
        Some(Mark {
            index,
            character,
            depth,
            stray,
        })
    }
}

/// The byte index of the first `wanted` in `text` outside every bracket
/// of `kinds`.
pub(super) fn find_outside(text: &str, kinds: &'static [Kind], wanted: char) -> Option<usize> {
    Scanner::new(text, kinds)
        .find(|mark| mark.character == wanted && mark.depth == 0 && !mark.stray)
        .map(|mark| mark.index)
}

/// The byte index of the `)` that closes the `(` at `open`, if any.
pub(super) fn closing_paren(text: &str, open: usize) -> Option<usize> {
    // Read from after the `(`, the `)` that closes it is the first with no opener of its own.
    Scanner::new(&text[open + 1..], PARENS)
        .find(|mark| mark.character == ')' && mark.stray)
        .map(|mark| open + 1 + mark.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bracket_has_the_depth_of_the_text_around_it() {
        let depths: Vec<usize> = Scanner::new("a(b)c", PARENS)
            .map(|mark| mark.depth)
            .collect();
        assert_eq!(depths, [0, 0, 1, 0, 0]);
    }

    #[test]
    fn a_closer_matches_only_an_opener_of_its_kind() {
        let mut scanner = Scanner::new("(]", ALL);
        let marks: Vec<Mark> = (&mut scanner).collect();
        assert!(marks[1].stray);
        assert_eq!(scanner.unclosed(), Some(('(', 0)));
    }

    #[test]
    fn unclosed_gives_the_earliest_opener_of_the_first_kind() {
        let mut scanner = Scanner::new("[a (b (c <d", ALL);
        scanner.by_ref().for_each(drop);
        assert_eq!(scanner.unclosed(), Some(('(', 3)));
    }

    #[test]
    fn only_the_kinds_asked_for_nest() {
        assert_eq!(find_outside("[a=b] = c", PARENS, '='), Some(2));
        assert_eq!(find_outside("[a=b] = c", ALL, '='), Some(6));
        assert_eq!(find_outside("f(a=b)", PARENS, '='), None);
    }

    #[test]
    fn closing_paren_skips_nested_pairs() {
        let text = "f(a, g(b)) -> c";
        assert_eq!(closing_paren(text, 1), Some(9));
        assert_eq!(closing_paren("f(a", 1), None);
    }
}
