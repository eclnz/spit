//! Suggested source path rules for files no rule matches. Files are
//! grouped by shape: the same text between their words, and the same
//! extension. The words that differ between a group's files become its
//! dimensions, named by the key before them where the path has one, as
//! `sub` in `sub-01`, and every rule is checked against its files before
//! it is suggested.

use std::collections::{BTreeMap, BTreeSet};

mod draft;
mod naming;
mod shape;

use self::draft::{escape, pieces, Draft};
use self::naming::name_sources;
use self::shape::Shape;
use super::pattern::{reach, NearestFile, Piece};
use crate::model::ProductDef;

/// A source for each group of files that share a shape, and the files that
/// share theirs with no other.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Suggestions {
    pub sources: Vec<SuggestedSource>,
    /// Files no suggested rule matches, but one matches the start of, as a
    /// `.bak` copy or a file with one more entity than its neighbours.
    pub near: Vec<NearlyMatched>,
    /// Files no other file shares a shape with, each of which could be a
    /// source with no dimensions, and files no rule can be written for,
    /// such as one with a space in its path.
    pub alone: Vec<String>,
    /// Sources the pipeline declares without a path rule that no group of
    /// files fits on its own.
    pub unfitted: Vec<String>,
}

/// A file a suggested rule nearly matches, and where the two part.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NearlyMatched {
    /// The source, or companion source, whose rule it is.
    pub source: String,
    /// The rule, with its extension.
    pub rule: String,
    pub file: NearestFile,
}

/// One suggested source and its path rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuggestedSource {
    pub name: String,
    /// In the order the path gives them.
    pub dimensions: Vec<String>,
    /// The dimensions no word in the path names, called `dim1`, `dim2` and
    /// so on, which the user should rename.
    pub unnamed: Vec<String>,
    /// The path rule, relative to the root, with each file's extension.
    pub rule: String,
    pub files: usize,
    pub example: String,
    /// The dimensions the pipeline declares this source with, when it
    /// declares it without a path rule.
    pub declared: Option<Vec<String>>,
    /// How many files the rule matches beyond its own group's.
    pub overlaps: usize,
    /// For files that share a stem and differ by extension, as an image and
    /// its JSON, the companion sources: each one's name and extension.
    /// `rule` is then the stem, and `name` the group's. Empty for a source.
    pub members: Vec<(String, String)>,
    /// The values each dimension holds in the files, in the order of
    /// `dimensions`.
    pub values: Vec<Vec<String>>,
}

/// Suggest a source for each group of `files` that share a shape. A group
/// named for one of `declared`, the sources with no rule, or whose
/// dimensions are that source's, is suggested as that source. `taken`
/// holds the pipeline's other product names, which no new source takes.
pub(super) fn suggest(
    files: &[String],
    declared: &[&ProductDef],
    taken: &BTreeSet<&str>,
) -> Suggestions {
    let mut groups: BTreeMap<_, Vec<Shape<'_>>> = BTreeMap::new();
    let mut alone = Vec::new();
    for file in files {
        // A space or `#` would end the word or start a comment in a rule.
        if file.contains(|character: char| character.is_whitespace() || character == '#') {
            alone.push(file.clone());
            continue;
        }
        let shape = Shape::of(file);
        groups.entry(shape.key()).or_default().push(shape);
    }
    let mut drafts = Vec::new();
    for shapes in groups.into_values() {
        // A group whose rule SPIT would not read as written is split by the
        // words of its folders and names, so a stray folder loses only its
        // own files.
        let shapes: Vec<_> = shapes.iter().collect();
        match Draft::of(&shapes).filter(|_| shapes.len() > 1) {
            Some(draft) => without_strays(&shapes, draft, &mut drafts, &mut alone),
            None => {
                let mut finer: BTreeMap<_, Vec<&Shape<'_>>> = BTreeMap::new();
                for shape in &shapes {
                    finer.entry(shape.separators()).or_default().push(shape);
                }
                for shapes in finer.into_values() {
                    match Draft::of(&shapes).filter(|_| shapes.len() > 1) {
                        Some(draft) => without_strays(&shapes, draft, &mut drafts, &mut alone),
                        None => alone.extend(shapes.iter().map(|shape| shape.file.to_owned())),
                    }
                }
            }
        }
    }
    drafts.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then(a.rule.cmp(&b.rule)));
    let (sources, unfitted) = name_sources(drafts, declared, taken, files);
    alone.sort();
    let (near, alone) = near_misses(alone, &sources);
    Suggestions {
        sources,
        near,
        alone,
        unfitted,
    }
}

/// Push `draft`, the rule for `shapes`, unless a few of them are strays:
/// when at least three in four share a skeleton, and their own rule names
/// more of its dimensions, as `Subject{subject}/Visit{visit}` does beside
/// one `subject04/visit1`, each skeleton's files get their own rule, and a
/// skeleton of one file is left alone.
fn without_strays<'a>(
    shapes: &[&Shape<'a>],
    draft: Draft,
    drafts: &mut Vec<Draft>,
    alone: &mut Vec<String>,
) {
    let mut skeletons: BTreeMap<_, Vec<&Shape<'a>>> = BTreeMap::new();
    for &shape in shapes {
        skeletons.entry(shape.skeleton()).or_default().push(shape);
    }
    let most = skeletons.values().max_by_key(|shapes| shapes.len());
    let better = skeletons.len() > 1
        && most.is_some_and(|most| {
            most.len() * 4 >= shapes.len() * 3
                && Draft::of(most).is_some_and(|own| own.unnamed() < draft.unnamed())
        });
    if !better {
        drafts.push(draft);
        return;
    }
    for shapes in skeletons.into_values() {
        match Draft::of(&shapes).filter(|_| shapes.len() > 1) {
            Some(draft) => drafts.push(draft),
            None => alone.extend(shapes.iter().map(|shape| shape.file.to_owned())),
        }
    }
}

/// The files of `alone` a suggested rule nearly matches, each with the rule
/// that matches most of its start, then shares most of its ending, as
/// `_T1w.json`, and the files left.
fn near_misses(
    alone: Vec<String>,
    sources: &[SuggestedSource],
) -> (Vec<NearlyMatched>, Vec<String>) {
    let rules: Vec<(&str, String, Vec<Piece>)> = sources
        .iter()
        .flat_map(|source| {
            let rules: Vec<(&str, String)> = if source.members.is_empty() {
                vec![(source.name.as_str(), source.rule.clone())]
            } else {
                source
                    .members
                    .iter()
                    .map(|(name, extension)| {
                        (
                            name.as_str(),
                            format!("{}{}", source.rule, escape(extension)),
                        )
                    })
                    .collect()
            };
            rules.into_iter().filter_map(|(name, rule)| {
                let pieces = pieces(&rule)?;
                Some((name, rule, pieces))
            })
        })
        .collect();
    let mut near = Vec::new();
    let mut left = Vec::new();
    for file in alone {
        let mut best: Option<((usize, usize), &str, &str, String)> = None;
        for (name, rule, pieces) in &rules {
            let (matched, expected) = reach(pieces, &file);
            let ending = match pieces.last() {
                Some(Piece::Literal(last)) => shared_ending(last, &file),
                _ => 0,
            };
            let score = (matched, ending);
            if matched > 0 && best.as_ref().is_none_or(|(most, ..)| score > *most) {
                best = Some((score, name, rule, expected));
            }
        }
        match best {
            Some(((matched, _), source, rule, expected)) => near.push(NearlyMatched {
                source: source.to_owned(),
                rule: rule.to_owned(),
                file: NearestFile {
                    file,
                    matched,
                    expected,
                },
            }),
            None => left.push(file),
        }
    }
    (near, left)
}

/// How many bytes `literal` and `file` share at their ends.
fn shared_ending(literal: &str, file: &str) -> usize {
    literal
        .bytes()
        .rev()
        .zip(file.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count()
}
