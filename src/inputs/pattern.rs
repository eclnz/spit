//! Path rules as patterns: matching one against a file or directory, and
//! how near a file comes to a rule that matches none.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

use crate::model::ProductDef;
use crate::paths::{encode_component, error, PathError, PathPart, PathPlaceholder, PathTemplate};

/// One piece of a path rule: text written as is, or a dimension's value.
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

/// A source whose path rule matched no file under the scanned root, and the
/// file left unmatched that comes nearest to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissedSource {
    pub product: String,
    /// Whether the source reads folders, so its nearest is a folder.
    pub folder: bool,
    /// The rule as it was matched, with `{@product}` and `{@entities}`
    /// written out and its extension added.
    pub rule: String,
    pub nearest: Option<NearestFile>,
}

/// Where a file parts from a rule it does not match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NearestFile {
    pub file: String,
    /// How many bytes at the start of `file` the rule matches.
    pub matched: usize,
    /// The rest of the rule from there, empty when the file goes on past
    /// the rule's end.
    pub expected: String,
}

impl fmt::Display for MissedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.folder { "folder" } else { "file" };
        write!(
            f,
            "source `{}` matched no {kind}s with path rule `{}`",
            self.product, self.rule
        )?;
        let Some(near) = &self.nearest else {
            return Ok(());
        };
        write!(
            f,
            "\n  the nearest {kind} is `{}`\n  {}",
            near.file,
            near.parting(kind)
        )
    }
}

impl NearestFile {
    /// Where the file parts from the rule, as "after `sub-01_`, the file
    /// has `acq-2_T1w` where the rule has `T1w`"; `kind` is `file` or
    /// `folder`.
    pub fn parting(&self, kind: &str) -> String {
        let (start, rest) = self.file.split_at(self.matched);
        let after = if start.is_empty() {
            String::new()
        } else {
            format!("after `{start}`, ")
        };
        let expected = &self.expected;
        match (rest.is_empty(), expected.is_empty()) {
            (true, _) => format!("{after}the {kind} ends where the rule has `{expected}`"),
            (false, true) => format!("{after}the {kind} has `{rest}` where the rule ends"),
            (false, false) => {
                format!("{after}the {kind} has `{rest}` where the rule has `{expected}`")
            }
        }
    }
}

/// `product`'s rule, `pieces`, which matched no file, with the file of
/// `files` it comes nearest to: the one it matches furthest from the start,
/// then the one that shares most of the rule's ending. A file is near only
/// when the rule matches some of its start, or it ends with the rule's last
/// literal, such as `_bold.nii.gz`, in full. A `folder` source's `files`
/// are the folders under the root.
pub(super) fn missed_source(
    product: &str,
    pieces: &[Piece],
    files: &[String],
    folder: bool,
) -> MissedSource {
    let ending = match pieces.last() {
        Some(Piece::Literal(last)) => last.as_str(),
        _ => "",
    };
    let shared_ending = |file: &str| {
        file.bytes()
            .rev()
            .zip(ending.bytes().rev())
            .take_while(|(a, b)| a == b)
            .count()
    };
    let mut best: Option<(usize, usize, NearestFile)> = None;
    for file in files {
        let (matched, expected) = reach(pieces, file);
        let score = (matched, shared_ending(file));
        let near = matched > 0 || (!ending.is_empty() && score.1 == ending.len());
        if near && best.as_ref().is_none_or(|(a, b, _)| score > (*a, *b)) {
            let near = NearestFile {
                file: file.clone(),
                matched,
                expected,
            };
            best = Some((score.0, score.1, near));
        }
    }
    MissedSource {
        product: product.to_owned(),
        folder,
        rule: render(pieces),
        nearest: best.map(|(_, _, near)| near),
    }
}

/// How far `pieces` match `text` from its start, in bytes, and the rest of
/// the rule from there. A literal may match in part, up to a separator
/// (`/`, `_`, `-` or `.`), so the rest can start inside it: `_run-` against
/// `_task-` matches `_`, while `sub-` against `ses-` matches nothing.
///
/// Every way of splitting values is tried, from an explicit stack: unlike
/// `match_from`, this keeps no bindings, so a dimension written twice may
/// take two values. It only says where a file parts from a rule.
pub(super) fn reach(pieces: &[Piece], text: &str) -> (usize, String) {
    let mut furthest = (0, render(pieces));
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
                        format!("{}{}", &literal[shared..], render(&pieces[index + 1..]));
                    furthest = (offset + shared, expected);
                }
            }
            Piece::Value(_) => {
                let longest = rest
                    .find(|character: char| !is_value_character(character))
                    .unwrap_or(rest.len());
                if longest == 0 && offset > furthest.0 {
                    furthest = (offset, render(&pieces[index..]));
                }
                stack.extend((1..=longest).map(|end| (index + 1, offset + end)));
            }
        }
    }
    furthest
}

/// `pieces` written as a path rule.
fn render(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Literal(literal) => literal.clone(),
            Piece::Value(dimension) => format!("{{{dimension}}}"),
        })
        .collect()
}
