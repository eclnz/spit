//! Matching a path a scan finds against a path rule, and reading the
//! dimension values it binds.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::model::{EntityBinding, ProductDef};
use crate::paths::{
    decode_component, encode_component, error, PathError, PathPart, PathPlaceholder, PathTemplate,
};

/// The entities `bound` in `path`, decoded, or a note of why a value cannot
/// be read and the path is skipped.
pub(super) fn read_binding(
    path: &str,
    bound: BTreeMap<String, &str>,
) -> Result<EntityBinding, String> {
    bound
        .into_iter()
        .map(|(dimension, encoded)| match readable_value(encoded) {
            Ok(value) => Ok((dimension, value)),
            Err(reason) => Err(format!(
                "`{path}`: `{dimension}` value `{encoded}` {reason}"
            )),
        })
        .collect()
}

/// Decode a path component. It must be written exactly as SPIT would write
/// it, so the record's path is the file found.
fn readable_value(encoded: &str) -> Result<String, &'static str> {
    let value = decode_component(encoded).ok_or("is not valid `%XX` text")?;
    if encode_component(&value) != encoded {
        return Err("is not how SPIT writes a value, so a path made from it would differ");
    }
    if value
        .chars()
        .any(|character| character.is_whitespace() || ",[]=#".contains(character))
    {
        return Err("holds a space or one of `,[]=#`, which an inventory cannot");
    }
    Ok(value)
}

pub(super) enum Piece {
    Literal(String),
    /// One path component value, as `bind_path` encodes it.
    Value(String),
}

pub(super) fn path_pattern(
    template: &PathTemplate,
    product: &ProductDef,
) -> Result<Vec<Piece>, PathError> {
    let mut pieces = Vec::new();
    for part in template.parts() {
        match part {
            PathPart::Literal(value) => pieces.push(Piece::Literal(value.clone())),
            PathPart::Placeholder(PathPlaceholder::Product) => {
                pieces.push(Piece::Literal(product.name.replace("::", ".")));
            }
            PathPart::Placeholder(PathPlaceholder::Entities) => {
                if product.dimensions.is_empty() {
                    pieces.push(Piece::Literal("global".to_owned()));
                }
                for (index, dimension) in product.dimensions.iter().enumerate() {
                    let separator = if index == 0 { "" } else { "__" };
                    pieces.push(Piece::Literal(format!(
                        "{separator}{}=",
                        encode_component(dimension)
                    )));
                    pieces.push(Piece::Value(dimension.clone()));
                }
            }
            // A source is made in no stage, so its rule never binds
            // `{@stage}`; `inspect_paths` rejects such a rule first.
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                return Err(error(format!(
                    "path rule for source `{}` uses `{}`, but a source is not made in a stage",
                    product.name,
                    PathPlaceholder::Stage
                )))
            }
            PathPart::Placeholder(PathPlaceholder::Dimension(dimension)) => {
                pieces.push(Piece::Value(dimension.clone()));
            }
            // A source with dimensions has `{@labels}` written out, and
            // its groups resolved; `inspect_paths` rejects the rest first.
            PathPart::Placeholder(PathPlaceholder::Labels) => {
                return Err(error(format!(
                    "path rule for source `{}` uses `{}`, but the source has no dimensions",
                    product.name,
                    PathPlaceholder::Labels
                )))
            }
            PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
        }
    }
    Ok(pieces)
}

/// Match `text` against `pieces`, binding each dimension to its encoded
/// value; a dimension used twice must have the same value both times.
pub(super) fn match_pattern<'a>(
    pieces: &[Piece],
    text: &'a str,
) -> Option<BTreeMap<String, &'a str>> {
    // A match begins with a literal first piece and ends with a literal last
    // one, which rules out most patterns without searching.
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
        Piece::Value(dimension) => {
            if let Some(value) = bound.get(dimension).map(|value| &text[value.clone()]) {
                return rest.starts_with(value)
                    && match_from(pieces, index + 1, text, offset + value.len(), bound, failed);
            }
            let longest = rest
                .find(|character: char| !is_value_character(character))
                .unwrap_or(rest.len());
            if let Some(end) = forced_end(pieces.get(index + 1), rest, longest) {
                if end == 0 {
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
                    pieces
                        .iter()
                        .skip(index)
                        .any(|piece| matches!(piece, Piece::Value(name) if name == *dimension))
                })
                .map(|(_, value)| (value.start, value.end))
                .collect();
            let attempt = (index, offset, later);
            if failed.contains(&attempt) {
                return false;
            }
            let found = (1..=longest).any(|end| {
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

/// Whether a value can hold `character`: what `encode_component` keeps, and
/// the `%` of what it escapes.
///
/// Keep in step with `encode_component` in `paths/components.rs`: if a value
/// could hold a character this denies, `forced_end` would bind it too short
/// and discovery would miss files.
fn is_value_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '%'
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
