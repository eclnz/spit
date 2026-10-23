//! Scanning a root for the contexts and source files an input recipe describes.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::coverage::{apply_drops, EveryGroupDropped};
use super::exclusions::{Excluder, UnmatchedExclusion};
use super::pattern::{
    inherit_shapes, match_pattern, missed_source, path_pattern, MissedSource, OutputPaths, Piece,
};
use crate::model::{
    ArtifactInstance, ContextPattern, EntityBinding, InputRules, Pipeline, PipelineIndex,
    ProductDef, Removal, SourceInventory, SourceRecord,
};
use crate::paths::{
    decode_component, encode_component, error, inspect_paths, require_directory,
    validate_context_pattern, PathBinder, PathError, PathPart, PathPlaceholder, PathTemplate,
    Shape,
};

/// The source files found under a root, what the recipe's rules removed,
/// and files whose values cannot be read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Discovery {
    pub inventory: SourceInventory,
    /// Files under the root that match no source path rule, leaving out
    /// SPIT's own files and those at the pipeline's output paths.
    pub unmatched_files: Vec<String>,
    /// Each source whose path rule matched no file, with the nearest of
    /// the unmatched files.
    pub missed: Vec<MissedSource>,
    /// Each skipped file and why.
    pub skipped: Vec<String>,
    /// What each `exclude` rule removed, then each group a conditional `exclude` rule
    /// removed.
    pub removed: Vec<Removal>,
    /// The first `exclude` rule that matched nothing, which the input stage
    /// reports as an error.
    pub unmatched: Option<UnmatchedExclusion>,
    /// conditional `exclude` rules that removed every group of a grouping, which the input
    /// stage reports as an error.
    pub emptied: Option<EveryGroupDropped>,
}

/// Find source artifacts and contexts under `root` from their path rules.
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
        .context_paths
        .iter()
        .map(DiscoveryPattern::new)
        .collect::<Result<Vec<_>, _>>()?;
    let sources = source_patterns(pipeline)?;
    let listing = Listing::of(root)?;
    // Each source with its place in `sources`, as file and folder sources:
    // a file source's rule is matched against files, a folder source's
    // against folders.
    let (folder_sources, file_sources): (Vec<_>, Vec<_>) = sources
        .iter()
        .enumerate()
        .partition(|(_, source)| source.product.folder);
    let mut matched = vec![false; sources.len()];
    // A file in a source folder is read with its folder.
    let mut folders = FxHashSet::default();
    for directory in &listing.directories {
        if let Some((index, _)) = first_match(&folder_sources, directory) {
            matched[index] = true;
            folders.insert(directory.as_str());
        }
    }
    let outputs = OutputPaths::of(pipeline, &listing.directories);
    let mut discovery = Discovery::default();
    // SPIT's own files, such as a recipe kept in its dataset, and what an
    // earlier run of the pipeline wrote under the root are not data a rule
    // missed.
    for file in listing.files.iter().filter(|file| !is_spit_file(file)) {
        if let Some((index, _)) = first_match(&file_sources, file) {
            matched[index] = true;
        } else if !file
            .match_indices('/')
            .any(|(end, _)| folders.contains(&file[..end]))
        {
            match first_match(&folder_sources, file) {
                Some((_, source)) => discovery.skipped.push(format!(
                    "`{file}`: is a file, but source `{}` reads folders",
                    source.product.name
                )),
                None if !outputs.hold(file) => discovery.unmatched_files.push(file.clone()),
                None => {}
            }
        }
    }
    for directory in &listing.directories {
        if folders.contains(directory.as_str()) {
            continue;
        }
        if let Some((_, source)) = first_match(&file_sources, directory) {
            discovery.skipped.push(format!(
                "`{directory}`: is a folder, but source `{}` reads files; end its declaration with `/` to read folders",
                source.product.name
            ));
        }
    }
    // A folder source that found nothing is compared with the folders.
    discovery.missed = sources
        .iter()
        .zip(&matched)
        .filter(|(_, matched)| !**matched)
        .map(|(source, _)| {
            let (candidates, folder) = if source.product.folder {
                (&listing.directories, true)
            } else {
                (&discovery.unmatched_files, false)
            };
            missed_source(&source.product.name, &source.pieces, candidates, folder)
        })
        .collect();
    find_contexts(&directory_patterns, &listing.directories, &mut discovery);
    // Exclusions come first: an excluded context expects no files, and an
    // excluded file is neither found nor needed.
    let mut excluder = Excluder::new(&rules.exclusions);
    let inventory = &mut discovery.inventory;
    inventory
        .contexts
        .retain(|binding| !excluder.context(binding));
    find_source_files(
        [
            (&file_sources, &listing.files),
            (&folder_sources, &listing.directories),
        ],
        &mut excluder,
        &mut discovery,
    )?;
    // Every conditional `exclude` rule is judged once, against what was found: a file a
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
    sort_records(&sources, &mut discovery.inventory.artifacts);
    match excluder.finish() {
        Ok(excluded) => {
            discovery.removed.splice(0..0, excluded);
        }
        Err(unmatched) => discovery.unmatched = Some(unmatched),
    }
    Ok(discovery)
}

/// The files under `root` that no source rule of `pipeline` matches, sorted
/// and leaving out SPIT's own files and those at its output paths, with
/// the sources that have no rule, whose files they may be. Unlike
/// [`discover`], a source with no rule is not an error, and no other rule
/// is applied.
pub(super) fn unmatched_files<'a>(
    pipeline: &'a Pipeline,
    root: &Path,
) -> Result<(Vec<String>, Vec<&'a ProductDef>), PathError> {
    require_directory(root)?;
    let index = PipelineIndex::new(pipeline);
    let mut patterns = Vec::new();
    let mut without = Vec::new();
    for product in sources_of(pipeline) {
        match index.path_template_for(&product.name) {
            Some(template) => {
                patterns.push((product.folder, path_pattern(&template, product, None)?));
            }
            None => without.push(product),
        }
    }
    let matches = |folder: bool, path: &str| {
        patterns.iter().any(|(reads_folders, pieces)| {
            *reads_folders == folder && match_pattern(pieces, path).is_some()
        })
    };
    let listing = Listing::of(root)?;
    let outputs = OutputPaths::of(pipeline, &listing.directories);
    // As in `discover`, a file in a source folder is read with its folder,
    // and a file a folder source's rule matches is skipped, not missed.
    let folders: FxHashSet<_> = listing
        .directories
        .iter()
        .filter(|directory| matches(true, directory))
        .map(String::as_str)
        .collect();
    let files = listing
        .files
        .iter()
        .filter(|file| {
            !is_spit_file(file)
                && !matches(false, file)
                && !matches(true, file)
                && !file
                    .match_indices('/')
                    .any(|(end, _)| folders.contains(&file[..end]))
                && !outputs.hold(file)
        })
        .cloned()
        .collect();
    Ok((files, without))
}

/// The source products of `pipeline`: those no step makes, in the order
/// the pipeline declares them.
fn sources_of(pipeline: &Pipeline) -> impl Iterator<Item = &ProductDef> {
    let outputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| &invocation.outputs)
        .collect();
    pipeline
        .products
        .iter()
        .filter(move |product| !outputs.contains(&product.name))
}

/// A directory discovery rule, checked, as pieces to match.
struct DiscoveryPattern {
    pieces: Vec<Piece>,
}

impl DiscoveryPattern {
    fn new(rule: &ContextPattern) -> Result<Self, PathError> {
        validate_context_pattern(rule)?;
        let mut pieces: Vec<Piece> = rule
            .template
            .parts()
            .iter()
            .map(|part| match part {
                PathPart::Literal(value) => Ok(Piece::Literal(value.clone())),
                PathPart::Placeholder(PathPlaceholder::Dimension(name, shape)) => {
                    Ok(Piece::Value(name.clone(), *shape))
                }
                PathPart::Placeholder(placeholder) => Err(error(format!(
                    "context path uses reserved placeholder `{placeholder}`"
                ))),
                PathPart::Group(_) => unreachable!("a discovery pattern has no group"),
            })
            .collect::<Result<_, _>>()?;
        inherit_shapes(&mut pieces);
        Ok(Self { pieces })
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
    let index = PipelineIndex::new(pipeline);
    sources_of(pipeline)
        .map(|product| {
            let template = index.path_template_for(&product.name).ok_or_else(|| {
                error(format!(
                    "no path rule for source `{}`, so its files cannot be discovered",
                    product.name
                ))
            })?;
            let pieces = path_pattern(&template, product, None)?;
            Ok(SourcePattern { product, pieces })
        })
        .collect()
}

/// Bind each directory that a discovery rule matches, recording the
/// contexts each rule finds and skipping values that cannot be read.
fn find_contexts(patterns: &[DiscoveryPattern], directories: &[String], discovery: &mut Discovery) {
    let mut contexts = BTreeSet::new();
    for directory in directories {
        for pattern in patterns {
            let Some(bound) = match_pattern(&pattern.pieces, directory) else {
                continue;
            };
            match read_binding(directory, bound) {
                Ok(binding) => {
                    contexts.insert(binding.clone());
                }
                Err(note) => discovery.skipped.push(note),
            }
        }
    }
    discovery.inventory.contexts = contexts.into_iter().collect();
}

/// A source with its place in the pipeline's list of sources.
type Placed<'s, 'a> = (usize, &'s SourcePattern<'a>);

/// The first of `sources` whose path rule matches `path`.
fn first_match<'s, 'a>(sources: &[Placed<'s, 'a>], path: &str) -> Option<Placed<'s, 'a>> {
    sources
        .iter()
        .find(|(_, source)| match_pattern(&source.pieces, path).is_some())
        .copied()
}

/// Record each file that a file source's path rule matches, and each folder
/// that a folder source's matches, unless an `exclude` rule removes it or a
/// value cannot be read.
fn find_source_files(
    kinds: [(&[Placed<'_, '_>], &[String]); 2],
    excluder: &mut Excluder<'_>,
    discovery: &mut Discovery,
) -> Result<(), PathError> {
    for (sources, paths) in kinds {
        for path in paths {
            let found = source_record(sources, path, excluder, &mut discovery.skipped)?;
            discovery.inventory.artifacts.extend(found);
        }
    }
    let artifacts = &mut discovery.inventory.artifacts;
    artifacts.sort();
    artifacts.dedup();
    Ok(())
}

/// The record for `file`, a file or a folder, if a source's rule matches
/// it and no `exclude` rule removes it. It must match one source only.
fn source_record(
    sources: &[Placed<'_, '_>],
    file: &str,
    excluder: &mut Excluder<'_>,
    notes: &mut Vec<String>,
) -> Result<Option<SourceRecord>, PathError> {
    let mut matches = sources.iter().filter_map(|(_, source)| {
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
    let kind = if product.folder { "folder" } else { "file" };
    if excluder.artifact(name, &binding) {
        return Ok(None);
    }
    if let Some((other, _)) = matches.next() {
        return Err(error(format!(
            "{kind} `{file}` matches the path rules of both `{name}` and `{}`",
            other.name
        )));
    }
    Ok(Some(SourceRecord::new(name, binding).at(file)))
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
pub(super) fn readable_value(encoded: &str) -> Result<String, &'static str> {
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
    ///
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
    // The shapes each source's rule gives its dimensions, found once.
    let mut shapes: FxHashMap<String, Vec<(String, Shape)>> = FxHashMap::default();
    for record in &mut inventory.artifacts {
        if !shapes.contains_key(&record.product) {
            let found = binder
                .index()
                .path_template_for(&record.product)
                .map(|template| {
                    template
                        .shaped()
                        .into_iter()
                        .map(|(dimension, shape)| (dimension.to_owned(), shape))
                        .collect()
                })
                .unwrap_or_default();
            shapes.insert(record.product.clone(), found);
        }
        if let Some((dimension, shape)) =
            shapes[&record.product].iter().find(|(dimension, shape)| {
                record
                    .entities
                    .get(dimension)
                    .is_some_and(|value| !shape.matches(value))
            })
        {
            let value = record.entities.get(dimension).unwrap_or_default();
            return Err(error(format!(
                "source `{}` has `{dimension}={value}`, which is not a `{shape}`, as its path rule gives `{{{dimension}:{shape}}}`; its file would not be read",
                record.product
            )));
        }
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
