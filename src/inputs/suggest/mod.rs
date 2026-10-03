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

use self::draft::Draft;
use self::naming::name_sources;
use self::shape::Shape;
use crate::model::ProductDef;

/// A source for each group of files that share a shape, and the files that
/// share theirs with no other.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Suggestions {
    pub sources: Vec<SuggestedSource>,
    /// Files no other file shares a shape with, each of which could be a
    /// source with no dimensions, and files no rule can be written for,
    /// such as one with a space in its path.
    pub alone: Vec<String>,
    /// Sources the pipeline declares without a path rule that no group of
    /// files fits on its own.
    pub unfitted: Vec<String>,
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
    /// its JSON, the `sidecars` members: each one's name and extension.
    /// `rule` is then the stem, and `name` the group's. Empty for a source.
    pub members: Vec<(String, String)>,
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
        let mut finer: BTreeMap<_, Vec<&Shape<'_>>> = BTreeMap::new();
        match Draft::of(&shapes.iter().collect::<Vec<_>>()) {
            Some(draft) if shapes.len() > 1 => drafts.push(draft),
            _ => {
                for shape in &shapes {
                    finer.entry(shape.separators()).or_default().push(shape);
                }
            }
        }
        for shapes in finer.into_values() {
            match Draft::of(&shapes).filter(|_| shapes.len() > 1) {
                Some(draft) => drafts.push(draft),
                None => alone.extend(shapes.iter().map(|shape| shape.file.to_owned())),
            }
        }
    }
    drafts.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then(a.rule.cmp(&b.rule)));
    let (sources, unfitted) = name_sources(drafts, declared, taken, files);
    alone.sort();
    Suggestions {
        sources,
        alone,
        unfitted,
    }
}
