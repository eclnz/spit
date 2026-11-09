//! A product's path template compiled once into a [`PathPattern`], which
//! both writes a binding's path and reads a path back to its binding, and
//! the value-character rule the two share.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

use super::path::{product_text, PathPart, PathPlaceholder, PathTemplate};
use super::shape::Shape;

/// Whether a value writes `byte` as it is in a path. Every other byte is
/// written `%XX`. Encoding, matching and the `default entities:` separator
/// check all read this.
pub(crate) fn is_kept(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

/// Whether an encoded value can hold `character`: a kept byte, or the `%`
/// of an escaped one. A pattern binds a value without searching when the
/// character after it is not one of these.
fn is_value_character(character: char) -> bool {
    u8::try_from(character).is_ok_and(|byte| is_kept(byte) || byte == b'%')
}

/// `value` as one path component, every byte [`is_kept`] denies as `%XX`.
pub(crate) fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    push_encoded(&mut encoded, value);
    encoded
}

/// Add `value` to `text` as [`encode_component`] encodes it. Runs of
/// bytes kept as they are are added whole.
pub(crate) fn push_encoded(text: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut plain = 0;
    for (index, byte) in value.bytes().enumerate() {
        if is_kept(byte) {
            continue;
        }
        // A run kept as it is is ASCII, so it starts and ends between
        // characters; an empty one may not.
        if plain < index {
            text.push_str(&value[plain..index]);
        }
        text.push('%');
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0xF)]));
        plain = index + 1;
    }
    text.push_str(&value[plain..]);
}

pub(crate) fn decode_component(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = encoded.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// One piece of a pattern: text written as is, or a dimension's value.
#[derive(Clone, Debug)]
enum Piece {
    Literal(String),
    Value {
        dimension: String,
        /// The shape this placeholder is written with, which errors show.
        written: Option<Shape>,
        /// The shape any placeholder of the dimension is written with, so
        /// `{id:digits}/{id}` reads `id` as digits both times. A template
        /// has one shape per dimension at most.
        shape: Option<Shape>,
    },
}

/// A placeholder a product's pattern cannot write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Gap {
    /// `{@entities}`, left in a product with no dimensions or no spelling.
    Entities,
    /// `{@stage}`, for a product no stage makes.
    Stage,
}

/// Why a binding has no path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RenderError {
    /// The pattern reaches a placeholder it cannot write.
    Gap(Gap),
    /// The binding has no value for this placeholder's dimension.
    Absent(PathPlaceholder),
}

/// A product's completed path template, with `{@product}` and `{@stage}`
/// written out. Rendering and matching read the same pieces, so a path
/// [`PathPattern::render`] writes, [`PathPattern::match_path`] reads back.
#[derive(Clone, Debug)]
pub(crate) struct PathPattern {
    /// The template's pieces, up to [`PathPattern::gap`].
    pieces: Vec<Piece>,
    /// The first placeholder the pattern cannot write, if any. Matching
    /// needs none; rendering fails on reaching it.
    pub(crate) gap: Option<Gap>,
    /// About how long a rendered path is.
    capacity: usize,
}

impl PathPattern {
    /// `template` compiled for `product`. `stage` is the directories
    /// `{@stage}` gives it, `None` outside every stage.
    pub(crate) fn new(template: &PathTemplate, product: &str, stage: Option<&str>) -> Self {
        let mut pieces = Vec::with_capacity(template.parts().len());
        let mut gap = None;
        for part in template.parts() {
            let piece = match part {
                PathPart::Literal(value) => Piece::Literal(value.clone()),
                PathPart::Placeholder(PathPlaceholder::Product) => {
                    Piece::Literal(product_text(product).into_owned())
                }
                PathPart::Placeholder(PathPlaceholder::Stage) => match stage {
                    Some(stage) => Piece::Literal(stage.to_owned()),
                    None => {
                        gap = Some(Gap::Stage);
                        break;
                    }
                },
                PathPart::Placeholder(PathPlaceholder::Entities) => {
                    gap = Some(Gap::Entities);
                    break;
                }
                PathPart::Placeholder(PathPlaceholder::Dimension(dimension, shape)) => {
                    Piece::Value {
                        dimension: dimension.clone(),
                        written: *shape,
                        shape: *shape,
                    }
                }
                PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
            };
            pieces.push(piece);
        }
        let shapes: Vec<(String, Shape)> = pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Value {
                    dimension,
                    shape: Some(shape),
                    ..
                } => Some((dimension.clone(), *shape)),
                _ => None,
            })
            .collect();
        for piece in &mut pieces {
            if let Piece::Value {
                dimension,
                shape: shape @ None,
                ..
            } = piece
            {
                *shape = shapes
                    .iter()
                    .find(|(shaped, _)| shaped == dimension)
                    .map(|(_, shape)| *shape);
            }
        }
        Self {
            pieces,
            gap,
            capacity: template.text.len() + 32,
        }
    }

    /// The path for the binding whose value of each dimension `value`
    /// gives, each value encoded as one path component.
    pub(crate) fn render<'v>(
        &self,
        value: impl Fn(&str) -> Option<&'v str>,
    ) -> Result<String, RenderError> {
        let mut path = String::with_capacity(self.capacity);
        for piece in &self.pieces {
            match piece {
                Piece::Literal(literal) => path.push_str(literal),
                Piece::Value {
                    dimension, written, ..
                } => {
                    let value = value(dimension).ok_or_else(|| {
                        RenderError::Absent(PathPlaceholder::Dimension(dimension.clone(), *written))
                    })?;
                    push_encoded(&mut path, value);
                }
            }
        }
        match self.gap {
            Some(gap) => Err(RenderError::Gap(gap)),
            None => Ok(path),
        }
    }

    /// `text`'s value of each dimension, still encoded, if the pattern
    /// matches it whole. A dimension written twice has one value.
    pub(crate) fn match_path<'a>(&self, text: &'a str) -> Option<BTreeMap<String, &'a str>> {
        let pieces = self.pieces.as_slice();
        // A match begins with a literal first piece and ends with a literal
        // last one, which rules out most patterns without searching.
        if let Some(Piece::Literal(first)) = pieces.first() {
            if !text.starts_with(first.as_str()) {
                return None;
            }
        }
        if let Some(Piece::Literal(last)) = pieces.last() {
            if !text.ends_with(last.as_str()) {
                return None;
            }
        }
        let mut bound = BTreeMap::new();
        let mut failed = BTreeSet::new();
        match_from(pieces, 0, text, 0, &mut bound, &mut failed).then(|| {
            bound
                .into_iter()
                .map(|(dimension, value)| (dimension, &text[value]))
                .collect()
        })
    }

    /// The literal text of the pattern, piece by piece.
    pub(crate) fn literals(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Literal(literal) => Some(literal.as_str()),
            Piece::Value { .. } => None,
        })
    }

    /// The last piece's text, if it is literal, such as `_bold.nii.gz`.
    pub(crate) fn ending(&self) -> &str {
        match self.pieces.last() {
            Some(Piece::Literal(last)) => last,
            _ => "",
        }
    }

    /// How far the pattern matches `text` from its start, in bytes, and the
    /// rest of the rule from there. A literal may match in part, up to a
    /// separator (`/`, `_`, `-` or `.`), so the rest can start inside it:
    /// `_run-` against `_task-` matches `_`, while `sub-` against `ses-`
    /// matches nothing.
    ///
    /// Every way of splitting values is tried, from an explicit stack:
    /// unlike [`PathPattern::match_path`], this keeps no bindings, so a
    /// dimension written twice may take two values. It only says where a
    /// file parts from a rule.
    pub(crate) fn reach(&self, text: &str) -> (usize, String) {
        let pieces = self.pieces.as_slice();
        let mut furthest = (0, written(pieces));
        let mut seen = BTreeSet::new();
        let mut stack = vec![(0, 0)];
        while let Some((index, offset)) = stack.pop() {
            if !seen.insert((index, offset)) {
                continue;
            }
            let rest = &text[offset..];
            let Some(piece) = pieces.get(index) else {
                if offset > furthest.0 {
                    furthest = (offset, String::new());
                }
                continue;
            };
            match piece {
                Piece::Literal(literal) if rest.starts_with(literal.as_str()) => {
                    stack.push((index + 1, offset + literal.len()));
                }
                Piece::Literal(literal) => {
                    let common: usize = literal
                        .chars()
                        .zip(rest.chars())
                        .take_while(|(a, b)| a == b)
                        .map(|(a, _)| a.len_utf8())
                        .sum();
                    let shared = literal[..common]
                        .rfind(['/', '_', '-', '.'])
                        .map_or(0, |separator| separator + 1);
                    if offset + shared > furthest.0 {
                        let expected =
                            format!("{}{}", &literal[shared..], written(&pieces[index + 1..]));
                        furthest = (offset + shared, expected);
                    }
                }
                Piece::Value { shape, .. } => {
                    let longest = longest_value(rest);
                    let mut ends = value_ends(*shape, rest, longest).peekable();
                    // No value here: the text has none, or none of the shape.
                    if ends.peek().is_none() && offset > furthest.0 {
                        furthest = (offset, written(&pieces[index..]));
                    }
                    stack.extend(ends.map(|end| (index + 1, offset + end)));
                }
            }
        }
        furthest
    }
}

/// Reads as a path rule, each dimension with the shape it is matched with.
impl fmt::Display for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&written(&self.pieces))
    }
}

/// `pieces` written as a path rule.
fn written(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Literal(literal) => literal.clone(),
            Piece::Value {
                dimension,
                shape: None,
                ..
            } => format!("{{{dimension}}}"),
            Piece::Value {
                dimension,
                shape: Some(shape),
                ..
            } => format!("{{{dimension}:{shape}}}"),
        })
        .collect()
}

/// The length of the longest value at the start of `rest`.
fn longest_value(rest: &str) -> usize {
    rest.find(|character: char| !is_value_character(character))
        .unwrap_or(rest.len())
}

/// A position that failed to match: the piece, the offset in the text, and
/// the values bound for dimensions that later pieces repeat.
type Attempt = (usize, usize, Vec<(usize, usize)>);

/// Match `pieces[index..]` against `text[offset..]`, binding each dimension
/// to the byte range of its value in `text`. Where a value's end is not
/// forced, failed positions are remembered, which keeps ambiguous splits
/// from taking exponential time.
fn match_from(
    pieces: &[Piece],
    index: usize,
    text: &str,
    offset: usize,
    bound: &mut BTreeMap<String, Range<usize>>,
    failed: &mut BTreeSet<Attempt>,
) -> bool {
    let rest = &text[offset..];
    let Some(piece) = pieces.get(index) else {
        return rest.is_empty();
    };
    match piece {
        Piece::Literal(literal) => {
            rest.starts_with(literal.as_str())
                && match_from(
                    pieces,
                    index + 1,
                    text,
                    offset + literal.len(),
                    bound,
                    failed,
                )
        }
        Piece::Value {
            dimension, shape, ..
        } => {
            if let Some(value) = bound.get(dimension).map(|value| &text[value.clone()]) {
                return rest.starts_with(value)
                    && match_from(pieces, index + 1, text, offset + value.len(), bound, failed);
            }
            let longest = longest_value(rest);
            if let Some(end) = forced_end(pieces.get(index + 1), rest, longest) {
                if end == 0 || !shape.is_none_or(|shape| shape.matches(&rest[..end])) {
                    return false;
                }
                bound.insert(dimension.clone(), offset..offset + end);
                let found = match_from(pieces, index + 1, text, offset + end, bound, failed);
                if !found {
                    bound.remove(dimension);
                }
                return found;
            }
            let later: Vec<_> = bound
                .iter()
                .filter(|(dimension, _)| {
                    pieces.iter().skip(index).any(|piece| {
                        matches!(piece, Piece::Value { dimension: name, .. } if name == *dimension)
                    })
                })
                .map(|(_, value)| (value.start, value.end))
                .collect();
            let attempt = (index, offset, later);
            if failed.contains(&attempt) {
                return false;
            }
            let found = value_ends(*shape, rest, longest).any(|end| {
                bound.insert(dimension.clone(), offset..offset + end);
                match_from(pieces, index + 1, text, offset + end, bound, failed)
            });
            if !found {
                bound.remove(dimension);
                failed.insert(attempt);
            }
            found
        }
    }
}

/// The lengths, up to `longest`, a value at the start of `rest` may have
/// and keep to `shape`. A shape of one length has only that one, so the
/// value needs no search; any other is tested at each length.
fn value_ends(
    shape: Option<Shape>,
    rest: &str,
    longest: usize,
) -> impl Iterator<Item = usize> + '_ {
    let fixed = shape.and_then(Shape::fixed_length);
    let (first, last) = match fixed {
        Some(length) if length <= longest => (length, length),
        Some(_) => (1, 0),
        None => (1, longest),
    };
    (first..=last).filter(move |&end| shape.is_none_or(|shape| shape.matches(&rest[..end])))
}

/// The one length a value at the start of `rest` can have, when what
/// follows it decides: the rest of the text at the end of the pattern, or
/// its first `longest` characters before a literal that starts with a
/// character no value holds, since every shorter value leaves a value
/// character where the literal must start. `Some(0)` means none fits; `None`
/// means several lengths must be tried.
fn forced_end(next: Option<&Piece>, rest: &str, longest: usize) -> Option<usize> {
    match next {
        None => Some(if longest == rest.len() { longest } else { 0 }),
        Some(Piece::Literal(literal))
            if literal
                .chars()
                .next()
                .is_some_and(|first| !is_value_character(first)) =>
        {
            Some(longest)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_component, encode_component, is_value_character, PathPattern};
    use crate::template::{PathTemplate, Shape};

    #[test]
    fn components_keep_letters_digits_and_dashes_and_encode_the_rest() {
        assert_eq!(encode_component("sub-01"), "sub-01");
        assert_eq!(encode_component("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(encode_component("éa"), "%C3%A9a");
        assert_eq!(encode_component("__"), "%5F%5F");
        assert_eq!(encode_component(""), "");
        assert_eq!(decode_component("a%20b%2F%C3%A9").as_deref(), Some("a b/é"));
    }

    /// A pattern binds a value without searching when the character after
    /// it is no value character, so no shape may accept one.
    #[test]
    fn every_shape_accepts_only_value_characters() {
        let shaped = ["0", "07", "120", "1999", "2024", "2024-02-29", "2099-01-01"];
        for shape in Shape::ALL {
            for value in shaped.iter().filter(|value| shape.matches(value)) {
                assert!(value.chars().all(is_value_character), "{shape}: {value}");
            }
        }
    }

    #[test]
    fn a_rendered_path_matches_back_with_the_shape_written_once() {
        let template = PathTemplate::parse("{@product}/{id:digits}/{id}_x.txt").unwrap();
        let pattern = PathPattern::new(&template, "lib::raw", None);
        let path = pattern
            .render(|dimension| (dimension == "id").then_some("07"))
            .unwrap();
        assert_eq!(path, "lib.raw/07/07_x.txt");
        let bound = pattern.match_path(&path).unwrap();
        assert_eq!(bound.get("id").copied(), Some("07"));
        assert!(pattern.match_path("lib.raw/a7/a7_x.txt").is_none());
        assert_eq!(pattern.to_string(), "lib.raw/{id:digits}/{id:digits}_x.txt");
    }
}
