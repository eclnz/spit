//! A group's suggested rule, before its source is named: the words that
//! stay the same are text, those that differ are dimensions, and the rule
//! is checked against the group's files with the matcher discovery uses.

use std::collections::BTreeSet;

use super::shape::{Component, Shape};
use crate::inputs::discover::readable_value;
use crate::inputs::pattern::{match_pattern, Piece};
use crate::paths::{PathPart, PathPlaceholder, PathTemplate};

/// A dimension of a group: what names it, and its value in each file.
pub(super) struct Dimension {
    pub(super) name: Option<String>,
    pub(super) values: Vec<String>,
}

/// One piece of a suggested rule.
enum Part {
    Text(String),
    Dimension(usize),
}

/// A group's rule before its source is named.
pub(super) struct Draft {
    pub(super) files: Vec<String>,
    pub(super) dimensions: Vec<Dimension>,
    parts: Vec<Part>,
    pub(super) rule: String,
    /// Words of the file name that stay the same, the last most telling.
    pub(super) name_words: Vec<String>,
    /// The same, in the folders.
    pub(super) folder_words: Vec<String>,
    pub(super) suffix: Option<String>,
    pub(super) extension: String,
}

impl Draft {
    /// The rule for `shapes`, which share a key, or `None` when SPIT would
    /// not read it as matching each of them with the values found.
    pub(super) fn of(shapes: &[&Shape<'_>]) -> Option<Self> {
        let first = shapes[0];
        let mut dimensions = Vec::new();
        let mut parts = Vec::new();
        let mut name_words = Vec::new();
        let mut folder_words = Vec::new();
        let last = first.components.len() - 1;
        for index in 0..=last {
            if index > 0 {
                parts.push(Part::Text("/".to_owned()));
            }
            let components: Vec<_> = shapes
                .iter()
                .map(|shape| &shape.components[index])
                .collect();
            let words = if index == last {
                &mut name_words
            } else {
                &mut folder_words
            };
            if components
                .iter()
                .all(|c| c.separators == components[0].separators)
            {
                read_words(&components, &mut parts, &mut dimensions, words);
            } else {
                // Folders or names of different forms, as `lr-high` and
                // `warmup`, are each one value.
                let values: Vec<String> = components.iter().map(|c| c.text.to_owned()).collect();
                if values
                    .iter()
                    .any(|value| !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                {
                    return None;
                }
                dimensions.push(Dimension { name: None, values });
                parts.push(Part::Dimension(dimensions.len() - 1));
            }
        }
        parts.push(Part::Text(first.extension.to_owned()));
        let (dimensions, parts) = merge_dimensions(dimensions, parts);
        let mut draft = Self {
            files: shapes.iter().map(|shape| shape.file.to_owned()).collect(),
            dimensions,
            parts,
            rule: String::new(),
            name_words,
            folder_words,
            suffix: first.suffix.map(str::to_owned),
            extension: first.extension.to_owned(),
        };
        name_unnamed(&mut draft.dimensions);
        draft.rule = draft.render();
        draft.matches_its_files().then_some(draft)
    }

    pub(super) fn render(&self) -> String {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => escape(text),
                Part::Dimension(index) => format!(
                    "{{{}}}",
                    self.dimensions[*index].name.as_deref().unwrap_or("")
                ),
            })
            .collect()
    }

    /// Whether SPIT, reading the rule, matches each of the group's files
    /// with the values found for it.
    fn matches_its_files(&self) -> bool {
        let Some(pieces) = pieces(&self.rule) else {
            return false;
        };
        self.files.iter().enumerate().all(|(index, file)| {
            match_pattern(&pieces, file).is_some_and(|bound| {
                self.dimensions.iter().all(|dimension| {
                    let name = dimension.name.as_deref().unwrap_or("");
                    bound.get(name).is_some_and(|value| {
                        *value == dimension.values[index] && readable_value(value).is_ok()
                    })
                })
            })
        })
    }

    /// The rule without the files' extension.
    pub(super) fn stem(&self) -> String {
        let extension = escape(&self.extension);
        self.rule
            .strip_suffix(extension.as_str())
            .unwrap_or(&self.rule)
            .to_owned()
    }

    pub(super) fn names(&self) -> Vec<String> {
        self.dimensions
            .iter()
            .map(|dimension| dimension.name.clone().unwrap_or_default())
            .collect()
    }
}

/// The rule's parts for one folder or name, which every file writes with
/// the same text between its words: words that stay the same are text, and
/// those that differ are a dimension. Words joined by `-` or nothing form
/// one unit, since a value may hold a `-`, as a date does; a unit that
/// starts with a word of letters that stays the same, as `sub-01` or
/// `wave3`, has that word as text and names the dimension after it, which
/// it keeps when its value has a digit, whether or not the value differs.
fn read_words(
    components: &[&Component<'_>],
    parts: &mut Vec<Part>,
    dimensions: &mut Vec<Dimension>,
    words: &mut Vec<String>,
) {
    let first = components[0];
    let constant = |position: usize| {
        components
            .windows(2)
            .all(|pair| pair[0].words[position] == pair[1].words[position])
    };
    let joined = |component: &Component<'_>, positions: &[usize]| {
        let mut text = String::new();
        for (index, &position) in positions.iter().enumerate() {
            if index > 0 {
                text.push_str(component.separators[position]);
            }
            text.push_str(component.words[position]);
        }
        text
    };
    let mut units: Vec<Vec<usize>> = Vec::new();
    for position in 0..first.words.len() {
        let joins = position > 0 && matches!(first.separators[position], "-" | "");
        match units.last_mut() {
            Some(unit) if joins => unit.push(position),
            _ => units.push(vec![position]),
        }
    }
    for unit in &units {
        parts.push(Part::Text(first.separators[unit[0]].to_owned()));
        let keyed = unit.len() > 1
            && constant(unit[0])
            && first.words[unit[0]]
                .chars()
                .all(|c| c.is_ascii_alphabetic());
        // A key with a number, as `ses-1` or `rev3`, names a dimension even
        // when every file has the same one, as a dataset of one session
        // does; a key with a word, as `task-rest`, stays as it is.
        let numbered = keyed
            && unit[1..]
                .iter()
                .any(|&position| first.words[position].contains(|c: char| c.is_ascii_digit()));
        let varying = if !numbered && unit.iter().all(|&position| constant(position)) {
            let text = joined(first, unit);
            if text.chars().any(|c| c.is_ascii_alphabetic()) {
                words.push(text.clone());
            }
            parts.push(Part::Text(text));
            continue;
        } else if keyed {
            parts.push(Part::Text(first.words[unit[0]].to_owned()));
            parts.push(Part::Text(first.separators[unit[1]].to_owned()));
            &unit[1..]
        } else {
            unit.as_slice()
        };
        let values = components.iter().map(|c| joined(c, varying)).collect();
        let name = keyed.then(|| first.words[unit[0]].to_ascii_lowercase());
        dimensions.push(Dimension { name, values });
        parts.push(Part::Dimension(dimensions.len() - 1));
    }
    parts.push(Part::Text(first.separators[first.words.len()].to_owned()));
}

/// One dimension for each that holds the same value in every file as an
/// earlier one with its name, or no name: a path that names `sub` in its
/// folder and its file name has one `sub`. A name that holds other values
/// in a later place takes a number.
fn merge_dimensions(dimensions: Vec<Dimension>, parts: Vec<Part>) -> (Vec<Dimension>, Vec<Part>) {
    let mut kept: Vec<Dimension> = Vec::new();
    let mut map = Vec::new();
    for dimension in dimensions {
        let same = kept.iter().position(|earlier| {
            earlier.values == dimension.values
                && (earlier.name.is_none()
                    || dimension.name.is_none()
                    || earlier.name == dimension.name)
        });
        match same {
            Some(index) => {
                if kept[index].name.is_none() {
                    kept[index].name = dimension.name;
                }
                map.push(index);
            }
            None => {
                map.push(kept.len());
                kept.push(dimension);
            }
        }
    }
    let mut used = BTreeSet::new();
    for dimension in &mut kept {
        if let Some(name) = &dimension.name {
            let mut unique = name.clone();
            let mut number = 2;
            while !used.insert(unique.clone()) {
                unique = format!("{name}{number}");
                number += 1;
            }
            dimension.name = Some(unique);
        }
    }
    let parts = parts
        .into_iter()
        .map(|part| match part {
            Part::Dimension(index) => Part::Dimension(map[index]),
            text => text,
        })
        .collect();
    (kept, parts)
}

/// Name each dimension no word names `dim1`, `dim2` and so on, skipping
/// names taken.
fn name_unnamed(dimensions: &mut [Dimension]) {
    let taken: BTreeSet<_> = dimensions.iter().filter_map(|d| d.name.clone()).collect();
    let mut number = 1;
    for dimension in dimensions.iter_mut().filter(|d| d.name.is_none()) {
        while taken.contains(&format!("dim{number}")) {
            number += 1;
        }
        dimension.name = Some(format!("dim{number}"));
        number += 1;
    }
}

/// `text` as a rule writes it: braces and brackets doubled.
fn escape(text: &str) -> String {
    text.replace('{', "{{")
        .replace('}', "}}")
        .replace('[', "[[")
        .replace(']', "]]")
}

/// The pieces SPIT matches for `rule`, which holds only text and
/// dimensions, or `None` when it does not parse.
pub(super) fn pieces(rule: &str) -> Option<Vec<Piece>> {
    let template = PathTemplate::parse(rule).ok()?;
    template
        .parts()
        .iter()
        .map(|part| match part {
            PathPart::Literal(text) => Some(Piece::Literal(text.clone())),
            PathPart::Placeholder(PathPlaceholder::Dimension(name)) => {
                Some(Piece::Value(name.clone()))
            }
            _ => None,
        })
        .collect()
}
