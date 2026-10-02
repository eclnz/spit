//! Scanning a root for the contexts and source files an input recipe describes.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::coverage::{apply_drops, DropIndex, EveryGroupDropped};
use super::exclusions::{Excluder, UnmatchedExclusion};
use super::pattern::{match_pattern, missed_source, path_pattern, MissedSource, Piece};
use crate::model::{
    ArtifactInstance, DirectoryDiscovery, EntityBinding, InputRules, Pipeline, PipelineIndex,
    ProductDef, Removal, SourceInventory, SourceRecord,
};
use crate::paths::{
    decode_component, encode_component, error, inspect_paths, require_directory,
    validate_discovery_rule, PathBinder, PathError, PathPart, PathPlaceholder, PathTemplate,
};

/// The source files found under a root, what the recipe's rules removed,
/// and files whose values cannot be read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Discovery {
    pub inventory: SourceInventory,
    /// Files under the root that match no source path rule.
    pub unmatched_files: Vec<String>,
    /// Each source whose path rule matched no file, with the nearest of
    /// the unmatched files.
    pub missed: Vec<MissedSource>,
    /// Each skipped file and why.
    pub skipped: Vec<String>,
    /// What each `exclude` rule removed, then each group a `drop` rule
    /// removed.
    pub removed: Vec<Removal>,
    /// The first `exclude` rule that matched nothing, which the input stage
    /// reports as an error.
    pub unmatched: Option<UnmatchedExclusion>,
    /// `drop` rules that removed every group of a grouping, which the input
    /// stage reports as an error.
    pub emptied: Option<EveryGroupDropped>,
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
/// `rules.source_paths` taking precedence, then `rules.source_default`.
pub fn discover_source_files(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<Discovery, PathError> {
    discover(
        &with_source_paths(pipeline, &rules.source_paths_for(pipeline)),
        rules,
        root,
    )
}

/// `pipeline` with `source_paths`, rules for its sources that it does not
/// set itself; borrowed when there are none.
pub(crate) fn with_source_paths<'a>(
    pipeline: &'a Pipeline,
    source_paths: &BTreeMap<String, PathTemplate>,
) -> Cow<'a, Pipeline> {
    if source_paths.is_empty() {
        return Cow::Borrowed(pipeline);
    }
    let mut merged = pipeline.clone();
    merged.product_paths.extend(source_paths.clone());
    Cow::Owned(merged)
}

/// Discover under `root` with `pipeline`, whose path rules already include
/// the recipe's source paths.
pub(super) fn discover(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<Discovery, PathError> {
    require_directory(root)?;
    inspect_paths(pipeline)?;
    let directory_patterns = rules
        .discoveries
        .iter()
        .map(DiscoveryPattern::new)
        .collect::<Result<Vec<_>, _>>()?;
    let sources = source_patterns(pipeline)?;
    let listing = Listing::of(root)?;
    let mut matched = vec![false; sources.len()];
    let mut unmatched_files = Vec::new();
    // SPIT's own files, such as a recipe kept in its dataset, are not data a
    // rule missed.
    for file in listing.files.iter().filter(|file| !is_spit_file(file)) {
        match sources
            .iter()
            .position(|source| match_pattern(&source.pieces, file).is_some())
        {
            Some(source) => matched[source] = true,
            None => unmatched_files.push(file.clone()),
        }
    }
    let missed = sources
        .iter()
        .zip(&matched)
        .filter(|(_, matched)| !**matched)
        .map(|(source, _)| missed_source(&source.product.name, &source.pieces, &unmatched_files))
        .collect();
    let mut discovery = Discovery {
        unmatched_files,
        missed,
        ..Discovery::default()
    };
    find_contexts(
        &directory_patterns,
        &listing.directories,
        root,
        &mut discovery,
    )?;
    // Exclusions come first: an excluded context expects no files, and an
    // excluded file is neither found nor needed.
    let mut excluder = Excluder::new(&rules.exclusions);
    let inventory = &mut discovery.inventory;
    inventory
        .contexts
        .retain(|binding| !excluder.context(binding));
    for bindings in inventory.discovered.values_mut() {
        bindings.retain(|binding| !excluder.context(binding));
    }
    let expected = expected_bindings(
        &sources,
        &directory_patterns,
        &discovery.inventory.discovered,
    );
    find_source_files(
        &sources,
        &listing.files,
        &expected,
        &mut excluder,
        &mut discovery,
    )?;
    // Every `drop` rule is judged once, against what was found: a file a
    // context expects but lacks counts as absent, so a rule can remove the
    // context rather than fail on the missing file below.
    let dropped = match apply_drops(rules, &mut discovery.inventory) {
        Ok(dropped) => dropped,
        Err(emptied) => {
            discovery.emptied = Some(emptied);
            return Ok(discovery);
        }
    };
    discovery
        .removed
        .extend(dropped.iter().map(|group| group.removal()));
    require_source_files(
        pipeline,
        root,
        &sources,
        &listing.files,
        &expected,
        &DropIndex::new(&dropped),
        &mut excluder,
    )?;
    sort_records(&sources, &mut discovery.inventory.artifacts);
    match excluder.finish() {
        Ok(excluded) => {
            discovery.removed.splice(0..0, excluded);
        }
        Err(unmatched) => discovery.unmatched = Some(unmatched),
    }
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
                PathPart::Group(_) => unreachable!("a discovery pattern has no group"),
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
    let index = PipelineIndex::new(pipeline);
    pipeline
        .products
        .iter()
        .filter(|product| !outputs.contains(&product.name))
        .map(|product| {
            let template = index.path_template_for(&product.name).ok_or_else(|| {
                error(format!(
                    "no path rule for source `{}`, so its files cannot be discovered",
                    product.name
                ))
            })?;
            let pieces = path_pattern(&template, product)?;
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

/// Record each file that a source's path rule matches, unless an `exclude`
/// rule removes it or a value cannot be read.
fn find_source_files(
    sources: &[SourcePattern<'_>],
    files: &[String],
    expected: &BTreeMap<&str, BTreeSet<EntityBinding>>,
    excluder: &mut Excluder<'_>,
    discovery: &mut Discovery,
) -> Result<(), PathError> {
    for file in files {
        let found = source_record(sources, file, expected, excluder, &mut discovery.skipped)?;
        discovery.inventory.artifacts.extend(found);
    }
    let artifacts = &mut discovery.inventory.artifacts;
    artifacts.sort();
    artifacts.dedup();
    Ok(())
}

/// The record for `file`, if a source's rule matches it and no `exclude`
/// rule removes it. A file must match one source only, and lie within the
/// contexts found for that source.
fn source_record(
    sources: &[SourcePattern<'_>],
    file: &str,
    expected: &BTreeMap<&str, BTreeSet<EntityBinding>>,
    excluder: &mut Excluder<'_>,
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
    let name = product.name.as_str();
    // An excluded file may lie outside every discovered context, such as a
    // misnamed copy of one that does.
    if excluder.artifact(name, &binding) {
        return Ok(None);
    }
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
/// except in groups a `drop` rule removed. An excluded binding needs no file;
/// when it has none, its exclusion is recorded here, since no file was found
/// to record it by.
fn require_source_files(
    pipeline: &Pipeline,
    root: &Path,
    sources: &[SourcePattern<'_>],
    files: &[String],
    expected: &BTreeMap<&str, BTreeSet<EntityBinding>>,
    dropped: &DropIndex<'_>,
    excluder: &mut Excluder<'_>,
) -> Result<(), PathError> {
    let mut binder = PathBinder::new(pipeline);
    for product in sources.iter().map(|source| source.product) {
        let bindings = expected.get(product.name.as_str()).into_iter().flatten();
        for binding in bindings.filter(|binding| !dropped.matches(binding)) {
            let excluded = excluder.excludes(&product.name, binding);
            let artifact = ArtifactInstance::new(
                &product.name,
                product.artifact_type.clone(),
                binding.clone(),
            );
            let relative = binder.bind(&product.dimensions, artifact.view(), || {
                format!("source `{artifact}`")
            })?;
            let full = root.join(&relative);
            // `is_file` alone accepts `s07.json` when only `S07.json` is
            // present on a case-insensitive filesystem. The listing keeps
            // each directory entry's actual spelling.
            let present = files.binary_search(&relative).is_ok() && full.is_file();
            if excluded {
                if !present {
                    excluder.artifact(&product.name, binding);
                }
                continue;
            }
            if !present {
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

/// Whether `file` is a pipeline, recipe, `.spitout` or `.spitdag`.
fn is_spit_file(file: &str) -> bool {
    Path::new(file).extension().is_some_and(|extension| {
        ["spit", "spitin", "spitout", "spitdag"]
            .iter()
            .any(|spit| extension == *spit)
    })
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
        listing.walk(root, None, "", &mut BTreeSet::new())?;
        listing.directories.sort();
        listing.files.sort();
        Ok(listing)
    }

    /// Add every directory and regular file under `directory`, following
    /// links. `visited` stops link cycles. Names that are not UTF-8 cannot
    /// match a rule.
    ///
    /// `canonical` is `directory` with every link resolved, when known. A
    /// listing gives each entry's type, and only a link changes where an
    /// entry is, so only a link is looked up and resolved; a directory
    /// inside is its directory's canonical path and its name.
    fn walk(
        &mut self,
        directory: &Path,
        canonical: Option<PathBuf>,
        prefix: &str,
        visited: &mut BTreeSet<PathBuf>,
    ) -> Result<(), PathError> {
        let unreadable = |reason: std::io::Error| {
            error(format!("cannot read `{}`: {reason}", directory.display()))
        };
        let canonical = match canonical {
            Some(canonical) => canonical,
            None => fs::canonicalize(directory).map_err(unreadable)?,
        };
        if !visited.insert(canonical.clone()) {
            return Ok(());
        }
        for entry in fs::read_dir(directory).map_err(unreadable)? {
            let entry = entry.map_err(unreadable)?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let relative = format!("{prefix}{name}");
            let path = entry.path();
            let (is_dir, is_file, linked) = match entry.file_type() {
                Ok(kind) if !kind.is_symlink() => (kind.is_dir(), kind.is_file(), false),
                _ => (path.is_dir(), path.is_file(), true),
            };
            if is_dir {
                let inner = (!linked).then(|| canonical.join(&name));
                self.walk(&path, inner, &format!("{relative}/"), visited)?;
                self.directories.push(relative);
            } else if is_file {
                self.files.push(relative);
            }
        }
        Ok(())
    }
}

/// Bind each source to its one declared path, so that resolving jobs needs
/// no rule for a source. `pipeline` has every source path rule: its own, the
/// recipe's and the inventory's. Older inventories may include record paths;
/// accept those only when they agree with the rule.
///
/// Keep in step with `as_read_back` in `parser/render_inventory.rs`, which relies
/// on every settled record's path being the one its rule gives: a `.spitout`
/// leaves such paths out, so a record allowed to keep another path would be
/// diagnosed in memory differently from its text.
pub(crate) fn locate_sources(
    pipeline: &Pipeline,
    inventory: &mut SourceInventory,
) -> Result<(), PathError> {
    let mut binder = PathBinder::new(pipeline);
    for record in &mut inventory.artifacts {
        if !binder.index().has_path(&record.product) {
            if record.path.is_some() {
                return Err(error(format!(
                    "source `{}` has a record path but no path rule",
                    record.product
                )));
            }
            continue;
        }
        let Some(product) = binder.index().product(&record.product) else {
            continue;
        };
        let artifact = ArtifactInstance::new(
            &product.name,
            product.artifact_type.clone(),
            record.entities.clone(),
        );
        let path = binder.bind(&product.dimensions, artifact.view(), || {
            format!("source `{artifact}`")
        })?;
        if let Some(given) = &record.path {
            if *given != path {
                return Err(error(format!(
                    "source `{artifact}` record path `{given}` differs from its path rule `{path}`"
                )));
            }
        }
        record.path = Some(path);
    }
    Ok(())
}
