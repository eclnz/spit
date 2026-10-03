//! Suggested source path rules for files no rule matches. Files are
//! grouped by shape: the same text between their words, and the same
//! extension. The words that differ between a group's files become its
//! dimensions, named by the key before them where the path has one, as
//! `sub` in `sub-01`, and every rule is checked against its files before
//! it is suggested.

use std::collections::{BTreeMap, BTreeSet};

use super::discover::readable_value;
use super::pattern::{match_pattern, Piece};
use crate::model::ProductDef;
use crate::paths::{PathPart, PathPlaceholder, PathTemplate};

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

/// A file's path as its folders and name, each as words and the text
/// around them, with its extension apart.
struct Shape<'a> {
    file: &'a str,
    /// Each folder, then the name without its extension.
    components: Vec<Component<'a>>,
    /// The name's text from its first `.`, as `.nii.gz`.
    extension: &'a str,
    /// The word after the last `_` of a BIDS file name, as `bold` in
    /// `sub-01_task-rest_bold`, which tells sources of one shape apart.
    suffix: Option<&'a str>,
}

/// A folder or name as words, each a run of letters and digits, and the
/// text between. A word of two or more letters then digits, as `wave3` or
/// `rev2`, is two words with nothing between, the letters naming the
/// digits; `s01` stays whole, as a value.
struct Component<'a> {
    text: &'a str,
    words: Vec<&'a str>,
    /// The text before each word, then the text after the last;
    /// `separators[i]` comes before `words[i]`.
    separators: Vec<&'a str>,
}

impl<'a> Component<'a> {
    fn of(text: &'a str) -> Self {
        let mut words = Vec::new();
        let mut separators = Vec::new();
        let mut start = 0;
        while let Some(begin) = text[start..].find(|c: char| c.is_ascii_alphanumeric()) {
            let begin = start + begin;
            let end = text[begin..]
                .find(|c: char| !c.is_ascii_alphanumeric())
                .map_or(text.len(), |length| begin + length);
            separators.push(&text[start..begin]);
            let word = &text[begin..end];
            let digits = word.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
            if digits >= 2
                && word[..digits].chars().all(|c| c.is_ascii_alphabetic())
                && word[digits..].chars().all(|c| c.is_ascii_digit())
            {
                words.push(&word[..digits]);
                separators.push("");
                words.push(&word[digits..]);
            } else {
                words.push(word);
            }
            start = end;
        }
        separators.push(&text[start..]);
        Self {
            text,
            words,
            separators,
        }
    }
}

impl<'a> Shape<'a> {
    fn of(file: &'a str) -> Self {
        let name_start = file.rfind('/').map_or(0, |slash| slash + 1);
        let body_end = file[name_start..]
            .find('.')
            .filter(|&dot| dot > 0)
            .map_or(file.len(), |dot| name_start + dot);
        Self {
            file,
            components: file[..body_end].split('/').map(Component::of).collect(),
            extension: &file[body_end..],
            suffix: bids_suffix(&file[name_start..body_end]),
        }
    }

    /// What files of one source share: how deep they are, their extension
    /// and BIDS suffix, and their top folder when it is a plain word, as
    /// `baseline` or `readings`, since such folders usually hold different
    /// kinds of data.
    fn key(&self) -> (usize, Option<&'a str>, &'a str, Option<&'a str>) {
        let top = self.components[0].text;
        let plain =
            self.components.len() > 1 && top.chars().all(|c| c.is_ascii_alphabetic() || c == '_');
        (
            self.components.len(),
            plain.then_some(top),
            self.extension,
            self.suffix,
        )
    }

    /// The text between the words of every component, which files of one
    /// shape share.
    fn separators(&self) -> Vec<&'a str> {
        let mut all = Vec::new();
        for component in &self.components {
            all.extend(&component.separators);
            all.push("/");
        }
        all
    }
}

/// The suffix of a BIDS-style name, `bold` in `sub-01_task-rest_bold`: the
/// last of its `_` parts, when an earlier part is a `key-value` entity.
fn bids_suffix(name: &str) -> Option<&str> {
    let (entities, suffix) = name.rsplit_once('_')?;
    let entity = |part: &str| {
        part.split_once('-').is_some_and(|(key, value)| {
            !key.is_empty()
                && key.chars().all(|c| c.is_ascii_alphabetic())
                && !value.is_empty()
                && value.chars().all(|c| c.is_ascii_alphanumeric())
        })
    };
    let is_word = !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphanumeric());
    (is_word && entities.split('_').any(entity)).then_some(suffix)
}

/// A dimension of a group: what names it, and its value in each file.
struct Dimension {
    name: Option<String>,
    values: Vec<String>,
}

/// One piece of a suggested rule.
enum Part {
    Text(String),
    Dimension(usize),
}

/// A group's rule before its source is named.
struct Draft {
    files: Vec<String>,
    dimensions: Vec<Dimension>,
    parts: Vec<Part>,
    rule: String,
    /// Words of the file name that stay the same, the last most telling.
    name_words: Vec<String>,
    /// The same, in the folders.
    folder_words: Vec<String>,
    suffix: Option<String>,
    extension: String,
}

impl Draft {
    /// The rule for `shapes`, which share a key, or `None` when SPIT would
    /// not read it as matching each of them with the values found.
    fn of(shapes: &[&Shape<'_>]) -> Option<Self> {
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

    fn render(&self) -> String {
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
    fn stem(&self) -> String {
        let extension = escape(&self.extension);
        self.rule
            .strip_suffix(extension.as_str())
            .unwrap_or(&self.rule)
            .to_owned()
    }

    fn names(&self) -> Vec<String> {
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
fn pieces(rule: &str) -> Option<Vec<Piece>> {
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

/// Name each draft's source: a declared source it fits, else a word from
/// its file names, unique among the pipeline's products and each other.
/// Drafts no declared source takes that share a stem and dimensions, as an
/// image and its JSON do, become one `sidecars` group. Also returns the
/// declared sources no draft fits.
fn name_sources(
    drafts: Vec<Draft>,
    declared: &[&ProductDef],
    taken: &BTreeSet<&str>,
    files: &[String],
) -> (Vec<SuggestedSource>, Vec<String>) {
    let mut chosen: Vec<(Draft, String, Option<&ProductDef>)> = drafts
        .into_iter()
        .map(|draft| {
            let word = draft
                .suffix
                .clone()
                .or_else(|| draft.name_words.last().cloned())
                .or_else(|| draft.folder_words.last().cloned())
                .unwrap_or_else(|| "files".to_owned());
            let name = identifier(&word);
            (draft, name, None)
        })
        .collect();
    // A declared source takes the one draft named for it, else the one whose
    // name starts with its own or its with the draft's, as `t1` and `t1w`,
    // else the one whose dimensions are its own.
    let mut unfitted = Vec::new();
    for product in declared {
        let wanted: BTreeSet<_> = product.dimensions.iter().cloned().collect();
        let fits = |test: usize, draft: &Draft, name: &str| match test {
            0 => name == product.name,
            1 => name.starts_with(&product.name) || product.name.starts_with(name),
            _ => draft.names().into_iter().collect::<BTreeSet<_>>() == wanted,
        };
        let fitting = (0..3).find_map(|test| {
            let fitting: Vec<_> = chosen
                .iter()
                .enumerate()
                .filter(|(_, (draft, name, owner))| owner.is_none() && fits(test, draft, name))
                .map(|(index, _)| index)
                .collect();
            (!fitting.is_empty()).then_some(fitting)
        });
        match fitting.as_deref() {
            Some(&[index]) => chosen[index].2 = Some(*product),
            _ => unfitted.push(product.name.clone()),
        }
    }
    let mut used: BTreeSet<String> = taken.iter().map(|name| (*name).to_owned()).collect();
    used.extend(declared.iter().map(|product| product.name.clone()));
    // Drafts by stem and dimensions, in the order they came, so that a
    // group of sidecars is suggested where its first member would be.
    let mut groups: Vec<Vec<(Draft, String, Option<&ProductDef>)>> = Vec::new();
    for entry in chosen {
        let together = groups.iter_mut().find(|group| {
            let (first, _, owner) = &group[0];
            owner.is_none()
                && entry.2.is_none()
                && first.stem() == entry.0.stem()
                && first.names() == entry.0.names()
        });
        match together {
            Some(group) => group.push(entry),
            None => groups.push(vec![entry]),
        }
    }
    let mut sources = Vec::new();
    for mut group in groups {
        let all: BTreeSet<&String> = group
            .iter()
            .flat_map(|(draft, _, _)| &draft.files)
            .collect();
        let overlaps = group
            .iter()
            .map(|(draft, _, _)| {
                let pieces = pieces(&draft.rule).unwrap_or_default();
                files
                    .iter()
                    .filter(|file| !all.contains(file) && match_pattern(&pieces, file).is_some())
                    .count()
            })
            .sum();
        let count = group.iter().map(|(draft, _, _)| draft.files.len()).sum();
        let sidecars = group.len() > 1;
        let (draft, name, product) = &mut group[0];
        let name = match product {
            Some(product) => {
                take_declared_names(draft, product);
                product.name.clone()
            }
            None if sidecars => unique_name(name, "", &mut used),
            None => unique_name(name, &draft.extension, &mut used),
        };
        let (draft, _, product) = &group[0];
        let members = if sidecars {
            group
                .iter()
                .map(|(member, _, _)| {
                    let suffix = identifier(member.extension.trim_start_matches('.'));
                    let member_name = unique_name(&format!("{name}_{suffix}"), "", &mut used);
                    (member_name, member.extension.clone())
                })
                .collect()
        } else {
            Vec::new()
        };
        let unnamed = draft
            .names()
            .into_iter()
            .filter(|name| is_placeholder_name(name) && product.is_none())
            .collect();
        sources.push(SuggestedSource {
            name,
            dimensions: draft.names(),
            unnamed,
            rule: if members.is_empty() {
                draft.rule.clone()
            } else {
                draft.stem()
            },
            files: count,
            example: draft.files[0].clone(),
            declared: product.map(|product| product.dimensions.clone()),
            overlaps,
            members,
        });
    }
    (sources, unfitted)
}

/// Whether `name` is one `name_unnamed` gave.
fn is_placeholder_name(name: &str) -> bool {
    name.strip_prefix("dim")
        .is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}

/// Give a draft for a declared source that source's dimension names, where
/// the two have as many: names the path gives stay, and the dimensions no
/// word names take the declared names left, in order.
fn take_declared_names(draft: &mut Draft, product: &ProductDef) {
    if draft.dimensions.len() != product.dimensions.len() {
        return;
    }
    let names = draft.names();
    let named: Vec<_> = names
        .iter()
        .filter(|name| !is_placeholder_name(name))
        .collect();
    if !named.iter().all(|name| product.dimensions.contains(name)) {
        return;
    }
    let mut left = product
        .dimensions
        .iter()
        .filter(|name| !named.contains(name));
    for dimension in &mut draft.dimensions {
        if dimension.name.as_deref().is_some_and(is_placeholder_name) {
            dimension.name = left.next().cloned();
        }
    }
    draft.rule = draft.render();
}

/// `word` as a product name: lowercase letters, digits and `_`, starting
/// with a letter.
fn identifier(word: &str) -> String {
    let mut name: String = word
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.insert_str(0, "files_");
    }
    name
}

/// `name`, or with the extension's last part then a number added, so that
/// it is in `used` once.
fn unique_name(name: &str, extension: &str, used: &mut BTreeSet<String>) -> String {
    let mut candidates = vec![name.to_owned()];
    if let Some(last) = extension.rsplit('.').next().filter(|last| !last.is_empty()) {
        candidates.push(format!("{name}_{}", identifier(last)));
    }
    let mut number = 2;
    loop {
        if let Some(found) = candidates
            .iter()
            .find(|candidate| !used.contains(*candidate))
        {
            used.insert(found.clone());
            return found.clone();
        }
        candidates = vec![format!("{name}_{number}")];
        number += 1;
    }
}
