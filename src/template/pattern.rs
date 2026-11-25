use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

use super::path::{product_text, PathPart, PathPlaceholder, PathTemplate};
use super::rule_text::push_escaped;
use super::shape::Shape;

pub(crate) fn is_kept(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

fn is_value_character(character: char) -> bool {
    u8::try_from(character).is_ok_and(|byte| is_kept(byte) || byte == b'%')
}

pub(crate) fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    push_encoded(&mut encoded, value);
    encoded
}

pub(crate) fn push_encoded(text: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut plain = 0;
    for (index, byte) in value.bytes().enumerate() {
        if is_kept(byte) {
            continue;
        }
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

#[derive(Clone, Debug)]
enum Piece {
    Literal(String),
    Wildcard,
    Value {
        dimension: String,
        written: Option<Shape>,
        shape: Option<Shape>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Gap {
    Entities,
    Stage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RenderError {
    Gap(Gap),
    Absent(PathPlaceholder),
    Reads,
}

#[derive(Clone, Debug)]
pub(crate) struct PathPattern {
    pieces: Vec<Piece>,
    pub(crate) gap: Option<Gap>,
    fixed: Vec<(String, String)>,
    capacity: usize,
}

impl PathPattern {
    pub(crate) fn new(template: &PathTemplate, product: &str, stage: Option<&str>) -> Self {
        let mut pieces = Vec::with_capacity(template.parts().len());
        let mut gap = None;
        let mut fixed = Vec::new();
        for part in template.parts() {
            let piece = match part {
                PathPart::Wildcard => Piece::Wildcard,
                PathPart::Fixed { dimension, value } => {
                    fixed.push((dimension.clone(), value.clone()));
                    continue;
                }
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
            fixed,
            capacity: template.text.len() + 32,
        }
    }

    pub(crate) fn fixed(&self) -> &[(String, String)] {
        &self.fixed
    }

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
                Piece::Wildcard => return Err(RenderError::Reads),
            }
        }
        if !self.fixed.is_empty() {
            return Err(RenderError::Reads);
        }
        match self.gap {
            Some(gap) => Err(RenderError::Gap(gap)),
            None => Ok(path),
        }
    }

    pub(crate) fn match_path<'a>(&self, text: &'a str) -> Option<BTreeMap<String, &'a str>> {
        let pieces = self.pieces.as_slice();
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

    pub(crate) fn literals(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Literal(literal) => Some(literal.as_str()),
            Piece::Value { .. } | Piece::Wildcard => None,
        })
    }

    pub(crate) fn ending(&self) -> &str {
        match self.pieces.last() {
            Some(Piece::Literal(last)) => last,
            _ => "",
        }
    }

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
                Piece::Wildcard => {
                    stack.extend((0..=name_end(rest)).map(|end| (index + 1, offset + end)));
                }
                Piece::Value { shape, .. } => {
                    let longest = longest_value(rest);
                    let mut ends = value_ends(*shape, rest, longest).peekable();
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

impl fmt::Display for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&written(&self.pieces))
    }
}

fn written(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Literal(literal) => {
                let mut text = String::with_capacity(literal.len());
                push_escaped(&mut text, literal);
                text
            }
            Piece::Wildcard => "*".to_owned(),
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

fn name_end(rest: &str) -> usize {
    rest.find('/').unwrap_or(rest.len())
}

fn longest_value(rest: &str) -> usize {
    rest.find(|character: char| !is_value_character(character))
        .unwrap_or(rest.len())
}

type Attempt = (usize, usize, Vec<(usize, usize)>);

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
        Piece::Wildcard => {
            let longest = name_end(rest);
            let forced = match pieces.get(index + 1) {
                None => Some(if longest == rest.len() {
                    longest
                } else {
                    usize::MAX
                }),
                Some(Piece::Literal(literal)) if literal.starts_with('/') => Some(longest),
                _ => None,
            };
            if let Some(end) = forced {
                return end != usize::MAX
                    && match_from(pieces, index + 1, text, offset + end, bound, failed);
            }
            let attempt = (index, offset, later_values(pieces, index, bound));
            if failed.contains(&attempt) {
                return false;
            }
            let found = (0..=longest)
                .any(|end| match_from(pieces, index + 1, text, offset + end, bound, failed));
            if !found {
                failed.insert(attempt);
            }
            found
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
            let attempt = (index, offset, later_values(pieces, index, bound));
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

fn later_values(
    pieces: &[Piece],
    index: usize,
    bound: &BTreeMap<String, Range<usize>>,
) -> Vec<(usize, usize)> {
    bound
        .iter()
        .filter(|(dimension, _)| {
            pieces.iter().skip(index).any(
                |piece| matches!(piece, Piece::Value { dimension: name, .. } if name == *dimension),
            )
        })
        .map(|(_, value)| (value.start, value.end))
        .collect()
}

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

    #[test]
    fn a_star_matches_any_text_within_one_name() {
        let template = PathTemplate::parse("*_{id:digits}_{visit}/*/anat_T1*").unwrap();
        let pattern = PathPattern::new(&template, "t1", None);
        let bound = pattern
            .match_path("ProjA_Case_4_A/Proj A - (1)/anat_T1_9")
            .unwrap();
        assert_eq!(bound.get("id").copied(), Some("4"));
        assert_eq!(bound.get("visit").copied(), Some("A"));
        assert!(pattern.match_path("x_4_A/a/b/anat_T1").is_none());
        assert!(pattern.match_path("_4_A//anat_T1").is_some());
        assert!(pattern.render(|_| Some("1")).is_err());
        assert_eq!(pattern.to_string(), "*_{id:digits}_{visit}/*/anat_T1*");
    }

    #[test]
    fn a_doubled_star_is_a_literal_star() {
        let template = PathTemplate::parse("a**b/{id}").unwrap();
        let pattern = PathPattern::new(&template, "s", None);
        assert!(pattern.match_path("a*b/1").is_some());
        assert!(pattern.match_path("axb/1").is_none());
        assert_eq!(pattern.render(|_| Some("1")).unwrap(), "a*b/1");
        assert_eq!(template.as_str(), "a**b/{id}");
    }

    #[test]
    fn a_fixed_value_matches_no_text_and_is_read_with_the_rest() {
        let template = PathTemplate::parse("*_Ctrl_{id}{cohort=ctrl}/x").unwrap();
        let pattern = PathPattern::new(&template, "s", None);
        assert!(pattern.match_path("P_Ctrl_6/x").is_some());
        assert_eq!(pattern.fixed(), [("cohort".to_owned(), "ctrl".to_owned())]);
    }

    #[test]
    fn stars_that_cannot_match_fail_quickly() {
        let rule = "*a*a*a*a*a*a*a*a*a*a*a*a*b";
        let template = PathTemplate::parse(rule).unwrap();
        let pattern = PathPattern::new(&template, "s", None);
        assert!(pattern.match_path(&"a".repeat(200)).is_none());
    }
}
