//! Scanning a root for the contexts and source files an input recipe describes.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::Path;

use super::coverage::{apply_skips, SkippedGroup};
use crate::model::{
    ArtifactInstance, DirectoryDiscovery, EntityBinding, InputRules, Pipeline, ProductDef,
    SourceInventory, SourceRecord,
};
use crate::paths::{
    bind_path, decode_component, encode_component, error, inspect_paths, validate_discovery_rule,
    PathError, PathPart, PathPlaceholder, PathTemplate,
};

/// The source files found under a root, and those skipped.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Discovery {
    pub inventory: SourceInventory,
    /// Each skipped file and why.
    pub skipped: Vec<String>,
}

/// Find source artifacts and contexts under `root`. Directory rules provide
/// contexts and require files for source products whose dimensions they cover;
/// other source products are found by matching their file path rules.
pub fn discover_sources(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<SourceInventory, PathError> {
    discover_source_files(pipeline, rules, root).map(|discovery| discovery.inventory)
}

/// As [`discover_sources`], also listing files that fit a rule but hold no
/// readable value.
///
/// Source files are found by the path rules of `pipeline`, with any in
/// `rules.source_paths` taking precedence.
pub fn discover_source_files(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<Discovery, PathError> {
    discover(&with_source_paths(pipeline, rules), rules, root)
}

/// `pipeline` with the path rules `rules` sets for its sources, which take
/// precedence; borrowed when there are none.
pub(super) fn with_source_paths<'a>(
    pipeline: &'a Pipeline,
    rules: &InputRules,
) -> Cow<'a, Pipeline> {
    if rules.source_paths.is_empty() {
        return Cow::Borrowed(pipeline);
    }
    let mut merged = pipeline.clone();
    merged.product_paths.extend(rules.source_paths.clone());
    Cow::Owned(merged)
}

/// Discover under `root` with `pipeline`, whose path rules already include
/// the recipe's source paths.
pub(super) fn discover(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<Discovery, PathError> {
    if !root.is_dir() {
        return Err(error(format!(
            "source root is not a directory: `{}`",
            root.display()
        )));
    }
    inspect_paths(pipeline)?;
    let directory_patterns = rules
        .discoveries
        .iter()
        .map(DiscoveryPattern::new)
        .collect::<Result<Vec<_>, _>>()?;
    let sources = source_patterns(pipeline)?;
    let listing = Listing::of(root)?;
    let mut discovery = Discovery::default();
    find_contexts(
        &directory_patterns,
        &listing.directories,
        root,
        &mut discovery,
    )?;
    let skipped = skip(rules, &mut discovery, true);
    let expected = expected_bindings(
        &sources,
        &directory_patterns,
        &discovery.inventory.discovered,
    );
    find_source_files(
        &sources,
        &listing.files,
        &skipped,
        &expected,
        &mut discovery,
    )?;
    let skipped = skip(rules, &mut discovery, false);
    require_source_files(pipeline, root, &sources, &expected, &skipped)?;
    sort_records(&sources, &mut discovery.inventory.artifacts);
    Ok(discovery)
}

/// A directory discovery rule, checked, as pieces to match.
struct DiscoveryPattern<'a> {
    rule: &'a DirectoryDiscovery,
    pieces: Vec<Piece>,
}

impl<'a> DiscoveryPattern<'a> {
    fn new(rule: &'a DirectoryDiscovery) -> Result<Self, PathError> {
        validate_discovery_rule(rule)?;
        let pieces = rule
            .template
            .parts()
            .iter()
            .map(|part| match part {
                PathPart::Literal(value) => Ok(Piece::Literal(value.clone())),
                PathPart::Placeholder(PathPlaceholder::Dimension(name)) => {
                    Ok(Piece::Value(name.clone()))
                }
                PathPart::Placeholder(placeholder) => Err(error(format!(
                    "discovery `{}` uses undeclared or reserved placeholder `{placeholder}`",
                    rule.name
                ))),
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { rule, pieces })
    }

    /// Whether this rule binds every dimension of `product`, so its contexts
    /// say which of the product's files must exist.
    fn covers(&self, product: &ProductDef) -> bool {
        product
            .dimensions
            .iter()
            .all(|dimension| self.rule.dimensions.contains(dimension))
    }
}

/// A source product and the pieces its path rule matches, in the order
/// the pipeline declares its products.
struct SourcePattern<'a> {
    product: &'a ProductDef,
    pieces: Vec<Piece>,
}

/// Every source of `pipeline` with its path pattern; each must have a rule.
fn source_patterns(pipeline: &Pipeline) -> Result<Vec<SourcePattern<'_>>, PathError> {
    let outputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| &invocation.outputs)
        .collect();
    pipeline
        .products
        .iter()
        .filter(|product| !outputs.contains(&product.name))
        .map(|product| {
            let template = pipeline.path_template_for(&product.name).ok_or_else(|| {
                error(format!(
                    "no path rule for source `{}`, so its files cannot be discovered",
                    product.name
                ))
            })?;
            let pieces = path_pattern(template, product)?;
            Ok(SourcePattern { product, pieces })
        })
        .collect()
}

/// Bind each directory that a discovery rule matches, recording the
/// contexts each rule finds and skipping values that cannot be read. Every
/// rule must match some directory.
fn find_contexts(
    patterns: &[DiscoveryPattern<'_>],
    directories: &[String],
    root: &Path,
    discovery: &mut Discovery,
) -> Result<(), PathError> {
    let mut contexts = BTreeSet::new();
    let mut found = vec![BTreeSet::new(); patterns.len()];
    for directory in directories {
        for (pattern, found) in patterns.iter().zip(&mut found) {
            let Some(bound) = match_pattern(&pattern.pieces, directory) else {
                continue;
            };
            match read_binding(directory, bound) {
                Ok(binding) => {
                    contexts.insert(binding.clone());
                    found.insert(binding);
                }
                Err(note) => discovery.skipped.push(note),
            }
        }
    }
    for (pattern, bindings) in patterns.iter().zip(&found) {
        if bindings.is_empty() {
            return Err(error(format!(
                "discovery `{}` matched no directories under `{}` with pattern `{}`",
                pattern.rule.name,
                root.display(),
                pattern.rule.template
            )));
        }
    }
    discovery.inventory.contexts = contexts.into_iter().collect();
    discovery.inventory.discovered = patterns
        .iter()
        .zip(found)
        .map(|(pattern, bindings)| (pattern.rule.name.clone(), bindings.into_iter().collect()))
        .collect();
    Ok(())
}

/// Apply the `skip` rules, of discovery rules only or of every rule, noting
/// each group they reject.
fn skip(rules: &InputRules, discovery: &mut Discovery, discovery_only: bool) -> Vec<SkippedGroup> {
    let skipped = apply_skips(rules, &mut discovery.inventory, discovery_only);
    discovery
        .skipped
        .extend(skipped.iter().map(SkippedGroup::note));
    skipped
}

/// For each source a discovery rule covers, the bindings its files must
/// have: the rule's contexts, cut to the source's dimensions. A source
/// whose covering rules found nothing is left out, so it is not checked.
fn expected_bindings<'a>(
    sources: &[SourcePattern<'a>],
    patterns: &[DiscoveryPattern<'_>],
    discovered: &BTreeMap<String, Vec<EntityBinding>>,
) -> BTreeMap<&'a str, BTreeSet<EntityBinding>> {
    let mut expected: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for source in sources {
        let dimensions = &source.product.dimensions;
        for pattern in patterns
            .iter()
            .filter(|pattern| pattern.covers(source.product))
        {
            let bindings = discovered.get(&pattern.rule.name).into_iter().flatten();
            expected
                .entry(source.product.name.as_str())
                .or_default()
                .extend(bindings.filter_map(|binding| binding.project(dimensions)));
        }
    }
    expected.retain(|_, bindings| !bindings.is_empty());
    expected
}

/// Record each file that a source's path rule matches, unless a skip rule
/// rejects its group or a value cannot be read.
fn find_source_files(
    sources: &[SourcePattern<'_>],
    files: &[String],
    skipped: &[SkippedGroup],
    expected: &BTreeMap<&str, BTreeSet<EntityBinding>>,
    discovery: &mut Discovery,
) -> Result<(), PathError> {
    for file in files {
        let found = source_record(sources, file, skipped, expected, &mut discovery.skipped)?;
        discovery.inventory.artifacts.extend(found);
    }
    let artifacts = &mut discovery.inventory.artifacts;
    artifacts.sort();
    artifacts.dedup();
    Ok(())
}

/// The record for `file`, if a source's rule matches it. A file must match
/// one source only, and lie within the contexts found for that source.
fn source_record(
    sources: &[SourcePattern<'_>],
    file: &str,
    skipped: &[SkippedGroup],
    expected: &BTreeMap<&str, BTreeSet<EntityBinding>>,
    notes: &mut Vec<String>,
) -> Result<Option<SourceRecord>, PathError> {
    let mut matches = sources.iter().filter_map(|source| {
        match_pattern(&source.pieces, file).map(|bound| (source.product, bound))
    });
    let Some((product, bound)) = matches.next() else {
        return Ok(None);
    };
    let binding = match read_binding(file, bound) {
        Ok(binding) => binding,
        Err(note) => {
            notes.push(note);
            return Ok(None);
        }
    };
    if skipped.iter().any(|group| group.matches(&binding)) {
        return Ok(None);
    }
    let name = product.name.as_str();
    if expected
        .get(name)
        .is_some_and(|bindings| !bindings.contains(&binding))
    {
        return Err(error(format!(
            "source file `{file}` for `{name}` lies outside the discovered contexts"
        )));
    }
    if let Some((other, _)) = matches.next() {
        return Err(error(format!(
            "file `{file}` matches the path rules of both `{name}` and `{}`",
            other.name
        )));
    }
    Ok(Some(SourceRecord::new(name, binding).at(file)))
}

/// Require the file of every source binding a discovery rule expects,
/// except in groups a skip rule rejected.
fn require_source_files(
    pipeline: &Pipeline,
    root: &Path,
    sources: &[SourcePattern<'_>],
    expected: &BTreeMap<&str, BTreeSet<EntityBinding>>,
    skipped: &[SkippedGroup],
) -> Result<(), PathError> {
    for product in sources.iter().map(|source| source.product) {
        let bindings = expected.get(product.name.as_str()).into_iter().flatten();
        for binding in
            bindings.filter(|binding| !skipped.iter().any(|group| group.matches(binding)))
        {
            let artifact = ArtifactInstance::new(
                &product.name,
                product.artifact_type.clone(),
                binding.clone(),
            );
            let relative = bind_path(pipeline, &product.dimensions, &artifact, || {
                format!("source `{artifact}`")
            })?;
            let full = root.join(&relative);
            if !full.is_file() {
                return Err(error(format!(
                    "missing source file for `{artifact}` at discovered context: `{}`",
                    full.display()
                )));
            }
        }
    }
    Ok(())
}

/// Order records by their product's place in the pipeline, then by its
/// dimensions, reading numbers as numbers.
fn sort_records(sources: &[SourcePattern<'_>], records: &mut [SourceRecord]) {
    let rank: BTreeMap<_, _> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let product = source.product;
            (
                product.name.as_str(),
                (index, product.dimensions.as_slice()),
            )
        })
        .collect();
    let rank = |record: &SourceRecord| {
        rank.get(record.product.as_str())
            .copied()
            .unwrap_or((usize::MAX, &[]))
    };
    records.sort_by(|left, right| {
        let (left_rank, dimensions) = rank(left);
        left_rank
            .cmp(&rank(right).0)
            .then_with(|| left.entities.cmp_in(&right.entities, dimensions))
    });
}

/// The entities `bound` in `path`, decoded, or a note of why a value cannot
/// be read and the path is skipped.
fn read_binding(path: &str, bound: BTreeMap<String, &str>) -> Result<EntityBinding, String> {
    bound
        .into_iter()
        .map(|(dimension, encoded)| match readable_value(encoded) {
            Ok(value) => Ok((dimension, value)),
            Err(reason) => Err(format!(
                "`{path}`: `{dimension}` value `{encoded}` {reason}"
            )),
        })
        .collect::<Result<_, _>>()
        .map(EntityBinding)
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

enum Piece {
    Literal(String),
    /// One path component value, as `bind_path` encodes it.
    Value(String),
}

fn path_pattern(template: &PathTemplate, product: &ProductDef) -> Result<Vec<Piece>, PathError> {
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
            // `{stage}`; `inspect_paths` rejects such a rule first.
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
        }
    }
    Ok(pieces)
}

/// Match `text` against `pieces`, binding each dimension to its encoded
/// value; a dimension used twice must have the same value both times.
fn match_pattern<'a>(pieces: &[Piece], text: &'a str) -> Option<BTreeMap<String, &'a str>> {
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
/// to the byte range of its value in `text`. Failed positions are
/// remembered, which keeps ambiguous splits from taking exponential time.
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
    let matched = match piece {
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
        Piece::Value(dimension) => match bound.get(dimension).map(|value| &text[value.clone()]) {
            Some(value) => {
                rest.starts_with(value)
                    && match_from(pieces, index + 1, text, offset + value.len(), bound, failed)
            }
            None => {
                let longest = rest
                    .find(|character: char| {
                        !(character.is_ascii_alphanumeric() || character == '-' || character == '%')
                    })
                    .unwrap_or(rest.len());
                let found = (1..=longest).any(|end| {
                    bound.insert(dimension.clone(), offset..offset + end);
                    match_from(pieces, index + 1, text, offset + end, bound, failed)
                });
                if !found {
                    bound.remove(dimension);
                }
                found
            }
        },
    };
    if !matched {
        failed.insert(attempt);
    }
    matched
}

/// The directories and files under a root, each sorted, as `/`-separated
/// paths relative to it.
#[derive(Default)]
struct Listing {
    directories: Vec<String>,
    files: Vec<String>,
}

impl Listing {
    fn of(root: &Path) -> Result<Self, PathError> {
        let mut listing = Self::default();
        listing.walk(root, "", &mut BTreeSet::new())?;
        listing.directories.sort();
        listing.files.sort();
        Ok(listing)
    }

    /// Add every directory and regular file under `directory`, following
    /// links. `visited` stops link cycles. Names that are not UTF-8 cannot
    /// match a rule.
    fn walk(
        &mut self,
        directory: &Path,
        prefix: &str,
        visited: &mut BTreeSet<std::path::PathBuf>,
    ) -> Result<(), PathError> {
        let unreadable = |reason: std::io::Error| {
            error(format!("cannot read `{}`: {reason}", directory.display()))
        };
        if !visited.insert(fs::canonicalize(directory).map_err(unreadable)?) {
            return Ok(());
        }
        for entry in fs::read_dir(directory).map_err(unreadable)? {
            let entry = entry.map_err(unreadable)?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let relative = format!("{prefix}{name}");
            let path = entry.path();
            if path.is_dir() {
                self.walk(&path, &format!("{relative}/"), visited)?;
                self.directories.push(relative);
            } else if path.is_file() {
                self.files.push(relative);
            }
        }
        Ok(())
    }
}

/// Give each record without a path the one its source's rule gives it, so
/// that resolving jobs needs no rule for a source. `pipeline` has the
/// recipe's source paths, from [`with_source_paths`]. A source with no rule
/// keeps no path.
pub(crate) fn locate_sources(
    pipeline: &Pipeline,
    inventory: &mut SourceInventory,
) -> Result<(), PathError> {
    for record in &mut inventory.artifacts {
        if record.path.is_some() || pipeline.path_template_for(&record.product).is_none() {
            continue;
        }
        let Some(product) = pipeline
            .products
            .iter()
            .find(|product| product.name == record.product)
        else {
            continue;
        };
        let artifact = ArtifactInstance::new(
            &product.name,
            product.artifact_type.clone(),
            record.entities.clone(),
        );
        let path = bind_path(pipeline, &product.dimensions, &artifact, || {
            format!("source `{artifact}`")
        })?;
        record.path = Some(path);
    }
    Ok(())
}
