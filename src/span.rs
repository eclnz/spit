//! Column ranges within a line of text, in bytes, for pointing at source.

use std::ops::Range;

/// A line and a byte range within it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Place {
    pub(crate) line: usize,
    pub(crate) columns: Range<usize>,
}

impl Place {
    pub(crate) fn new(line: usize, columns: Range<usize>) -> Self {
        Self { line, columns }
    }
}

/// The byte range `part` occupies in `line`, when `part` is a slice of `line`.
///
/// Parsing works on slices of the original line, so this recovers where a
/// token came from without the parser tracking offsets itself.
pub(crate) fn columns_of(line: &str, part: &str) -> Option<Range<usize>> {
    let start = (part.as_ptr() as usize).checked_sub(line.as_ptr() as usize)?;
    let end = start + part.len();
    (end <= line.len()).then_some(start..end)
}

/// The range of `line` without leading indentation, trailing space, or a
/// trailing comment.
pub(crate) fn content_columns(line: &str) -> Range<usize> {
    let content = crate::parser::strip_comment(line).trim();
    columns_of(line, content).unwrap_or(0..line.len())
}

/// The first occurrence of `word` in `line` at or after `from` that is not
/// part of a longer name.
pub(crate) fn find_word(line: &str, from: usize, word: &str) -> Option<Range<usize>> {
    if word.is_empty() {
        return None;
    }
    let is_name = |character: char| character.is_ascii_alphanumeric() || character == '_';
    let mut start = from;
    while let Some(offset) = line.get(start..)?.find(word) {
        let begin = start + offset;
        let end = begin + word.len();
        let before = line[..begin].chars().next_back();
        let after = line[end..].chars().next();
        if !before.is_some_and(is_name) && !after.is_some_and(is_name) {
            return Some(begin..end);
        }
        start = begin + word.len();
    }
    None
}

/// The UTF-16 code unit range of a byte range in `line`, as editors count.
pub(crate) fn utf16_columns(line: &str, columns: &Range<usize>) -> Range<usize> {
    let units = |end: usize| {
        line.get(..end.min(line.len()))
            .map_or(0, |prefix| prefix.encode_utf16().count())
    };
    units(columns.start)..units(columns.end)
}
