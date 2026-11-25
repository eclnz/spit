use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::coverage::{apply_drops, EveryGroupDropped};
use super::duplicates::one_record_each;
use super::exclusions::{Excluder, UnmatchedExclusion};
use super::pattern::{missed_source, path_pattern, MissedSource, OutputPaths};
use crate::model::{
    identity, ArtifactInstance, ContextPattern, EntityBinding, InputRules, Pipeline, PipelineIndex,
    Preference, ProductDef, Removal, SourceInventory, SourceRecord,
};
use crate::paths::{inspect_paths, validate_context_pattern, PathBinder};
use crate::template::{
    decode_component, encode_component, error, require_directory, PathError, PathPattern,
    PathTemplate, Shape,
};

const FINDER_ALIAS_HEADER: &[u8; 12] = b"book\0\0\0\0mark";

fn is_finder_alias(path: &Path) -> bool {
    let mut header = [0; FINDER_ALIAS_HEADER.len()];
    fs::File::open(path)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut header))
        .is_ok_and(|()| &header == FINDER_ALIAS_HEADER)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Discovery {
    pub inventory: SourceInventory,
    pub unmatched_files: Vec<String>,
    pub missed: Vec<MissedSource>,
    pub skipped: Vec<String>,
    pub removed: Vec<Removal>,
    pub unmatched: Option<UnmatchedExclusion>,
    pub emptied: Option<EveryGroupDropped>,
}

pub fn discover_sources(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<SourceInventory, PathError> {
    discover_source_files(pipeline, rules, root).map(|discovery| discovery.inventory)
}

pub fn discover_source_files(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
) -> Result<Discovery, PathError> {
    discover(
        &with_source_paths(pipeline, &rules.source_paths_for(pipeline)),
        rules,
        root,
        None,
    )
}

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

pub(super) fn discover(
    pipeline: &Pipeline,
    rules: &InputRules,
    root: &Path,
    outputs: Option<&Path>,
) -> Result<Discovery, PathError> {
    require_directory(root)?;
    inspect_paths(pipeline)?;
    let directory_patterns = rules
        .context_paths
        .iter()
        .map(DiscoveryPattern::new)
        .collect::<Result<Vec<_>, _>>()?;
    let sources = source_patterns(pipeline)?;
    let named_hidden = hidden_names(&sources, &directory_patterns);
    let written = outputs.and_then(|folder| fs::canonicalize(folder).ok());
    let mut listing = Listing::of(root, &named_hidden, written.as_deref())?;
    let (folder_sources, file_sources): (Vec<_>, Vec<_>) = sources
        .iter()
        .enumerate()
        .partition(|(_, source)| source.product.folder);
    let mut matched = vec![false; sources.len()];
    let mut folders = FxHashSet::default();
    for directory in &listing.directories {
        if let Some((index, _)) = first_match(&folder_sources, directory) {
            matched[index] = true;
            folders.insert(directory.as_str());
        }
    }
    let outputs = OutputPaths::of(pipeline, &listing.directories);
    listing.files.retain(|file| !outputs.beside_source(file));
    let mut discovery = Discovery::default();
    for file in listing.files.iter().filter(|file| !is_spit_file(file)) {
        if let Some((index, _)) = first_match(&file_sources, file) {
            matched[index] = true;
        } else if !file
            .match_indices('/')
            .any(|(end, _)| folders.contains(&file[..end]))
        {
            match first_match(&folder_sources, file) {
                Some(_) if is_finder_alias(&root.join(file)) => discovery.skipped.push(format!(
                    "`{file}`: it is a macOS Finder alias, which tools cannot follow; replace it with a symbolic link (ln -s TARGET {file})"
                )),
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
            missed_source(&source.product.name, &source.patterns, candidates, folder)
        })
        .collect();
    find_contexts(&directory_patterns, &listing.directories, &mut discovery);
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
        &rules.preferences,
        &mut discovery,
    )?;
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
    super::local::append(pipeline, root, &mut discovery)?;
    Ok(discovery)
}

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

struct DiscoveryPattern {
    pattern: PathPattern,
}

impl DiscoveryPattern {
    fn new(rule: &ContextPattern) -> Result<Self, PathError> {
        validate_context_pattern(rule)?;
        Ok(Self {
            pattern: PathPattern::new(&rule.template, "", None),
        })
    }
}

struct SourcePattern<'a> {
    product: &'a ProductDef,
    patterns: Vec<PathPattern>,
}

impl SourcePattern<'_> {
    fn first_match<'t>(&self, path: &'t str) -> Option<(usize, BTreeMap<String, &'t str>)> {
        self.patterns
            .iter()
            .enumerate()
            .find_map(|(rule, pattern)| Some((rule, pattern.match_path(path)?)))
    }
}

fn source_patterns(pipeline: &Pipeline) -> Result<Vec<SourcePattern<'_>>, PathError> {
    let index = PipelineIndex::new(pipeline);
    sources_of(pipeline)
        .filter(|product| product.here.is_none())
        .map(|product| {
            let template = index.path_template_for(&product.name).ok_or_else(|| {
                error(format!(
                    "no path rule for source `{}`, so its files cannot be discovered",
                    product.name
                ))
            })?;
            let patterns = template
                .rules()
                .map(|rule| path_pattern(rule, product, None))
                .collect::<Result<_, _>>()?;
            Ok(SourcePattern { product, patterns })
        })
        .collect()
}

fn find_contexts(patterns: &[DiscoveryPattern], directories: &[String], discovery: &mut Discovery) {
    let mut contexts = BTreeSet::new();
    for directory in directories {
        for pattern in patterns {
            let Some(bound) = pattern.pattern.match_path(directory) else {
                continue;
            };
            match read_binding(directory, bound, pattern.pattern.fixed()) {
                Ok(binding) => {
                    contexts.insert(binding.clone());
                }
                Err(note) => discovery.skipped.push(note),
            }
        }
    }
    discovery.inventory.contexts = contexts.into_iter().collect();
}

type Placed<'s, 'a> = (usize, &'s SourcePattern<'a>);

fn first_match<'s, 'a>(sources: &[Placed<'s, 'a>], path: &str) -> Option<Placed<'s, 'a>> {
    sources
        .iter()
        .find(|(_, source)| source.first_match(path).is_some())
        .copied()
}

fn find_source_files(
    kinds: [(&[Placed<'_, '_>], &[String]); 2],
    excluder: &mut Excluder<'_>,
    preferences: &[Preference],
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
    let folders: FxHashSet<&str> = kinds[1]
        .0
        .iter()
        .map(|(_, source)| source.product.name.as_str())
        .collect();
    one_record_each(artifacts, preferences, &folders, &mut discovery.removed)
}

fn source_record(
    sources: &[Placed<'_, '_>],
    file: &str,
    excluder: &mut Excluder<'_>,
    notes: &mut Vec<String>,
) -> Result<Option<SourceRecord>, PathError> {
    let mut matches = sources.iter().filter_map(|(_, source)| {
        source
            .first_match(file)
            .map(|(rule, bound)| (*source, rule, bound))
    });
    let Some((source, rule, bound)) = matches.next() else {
        return Ok(None);
    };
    let product = source.product;
    let pattern = &source.patterns[rule];
    let binding = match read_binding(file, bound, pattern.fixed()) {
        Ok(binding) => binding,
        Err(note) => {
            notes.push(note);
            return Ok(None);
        }
    };
    let name = product.name.as_str();
    let kind = if product.folder { "folder" } else { "file" };
    if excluder.artifact(name, &binding, Some(file)) {
        return Ok(None);
    }
    if let Some(other) = source.patterns[rule + 1..]
        .iter()
        .find(|other| other.match_path(file).is_some())
    {
        return Err(error(format!(
            "{kind} `{file}` matches two path rules of `{name}`, `{pattern}` and `{other}`; make them match different {kind}s"
        )));
    }
    if let Some((other, _, _)) = matches.next() {
        return Err(error(format!(
            "{kind} `{file}` matches the path rules of both `{name}` and `{}`",
            other.product.name
        )));
    }
    Ok(Some(SourceRecord::new(name, binding).at(file)))
}

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

fn read_binding(
    path: &str,
    bound: BTreeMap<String, &str>,
    fixed: &[(String, String)],
) -> Result<EntityBinding, String> {
    bound
        .into_iter()
        .map(|(dimension, encoded)| match readable_value(encoded) {
            Ok(value) => Ok((dimension, value)),
            Err(reason) => Err(format!(
                "`{path}`: `{dimension}` value `{encoded}` {reason}"
            )),
        })
        .chain(fixed.iter().cloned().map(Ok))
        .collect()
}

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

fn is_spit_file(file: &str) -> bool {
    Path::new(file).extension().is_some_and(|extension| {
        ["spit", "spitin", "spitout", "spitdag", "spitenv"]
            .iter()
            .any(|spit| extension == *spit)
    })
}

fn hidden_names(sources: &[SourcePattern], contexts: &[DiscoveryPattern]) -> BTreeSet<String> {
    let literals = sources
        .iter()
        .flat_map(|source| &source.patterns)
        .flat_map(PathPattern::literals)
        .chain(
            contexts
                .iter()
                .flat_map(|context| context.pattern.literals()),
        );
    let mut names = BTreeSet::new();
    for text in literals {
        names.extend(
            text.split('/')
                .filter(|part| part.starts_with('.'))
                .map(str::to_owned),
        );
    }
    names
}

#[derive(Default)]
struct Listing {
    directories: Vec<String>,
    files: Vec<String>,
}

impl Listing {
    fn of(
        root: &Path,
        named_hidden: &BTreeSet<String>,
        outputs: Option<&Path>,
    ) -> Result<Self, PathError> {
        let mut listing = Self::default();
        let walk = Walk {
            named_hidden,
            outputs,
        };
        listing.walk(root, None, "", &mut BTreeSet::new(), &walk)?;
        listing.directories.sort();
        listing.files.sort();
        Ok(listing)
    }

    fn walk(
        &mut self,
        directory: &Path,
        canonical: Option<PathBuf>,
        prefix: &str,
        visited: &mut BTreeSet<PathBuf>,
        walk: &Walk<'_>,
    ) -> Result<bool, PathError> {
        let unreadable = |reason: std::io::Error| {
            error(format!("cannot read `{}`: {reason}", directory.display()))
        };
        let gone = |reason: &std::io::Error| {
            !prefix.is_empty() && reason.kind() == std::io::ErrorKind::NotFound
        };
        let canonical = match canonical {
            Some(canonical) => canonical,
            None => match fs::canonicalize(directory) {
                Ok(canonical) => canonical,
                Err(reason) if gone(&reason) => return Ok(false),
                Err(reason) => return Err(unreadable(reason)),
            },
        };
        if !prefix.is_empty() && walk.outputs == Some(canonical.as_path()) {
            return Ok(false);
        }
        if !visited.insert(canonical.clone()) {
            return Ok(true);
        }
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(reason) if gone(&reason) => return Ok(false),
            Err(reason) => return Err(unreadable(reason)),
        };
        for entry in entries {
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
                if name.starts_with('.') && !walk.named_hidden.contains(&name) {
                    continue;
                }
                let inner = (!linked).then(|| canonical.join(&name));
                if self.walk(&path, inner, &format!("{relative}/"), visited, walk)? {
                    self.directories.push(relative);
                }
            } else if is_file {
                self.files.push(relative);
            }
        }
        Ok(true)
    }
}

struct Walk<'a> {
    named_hidden: &'a BTreeSet<String>,
    outputs: Option<&'a Path>,
}

pub(crate) fn locate_sources(
    pipeline: &Pipeline,
    inventory: &mut SourceInventory,
) -> Result<(), PathError> {
    let mut binder = PathBinder::new(pipeline);
    let mut shapes: FxHashMap<String, Vec<(String, Shape)>> = FxHashMap::default();
    let mut readers: FxHashMap<String, Option<Vec<PathPattern>>> = FxHashMap::default();
    for record in &mut inventory.artifacts {
        if !readers.contains_key(&record.product) {
            let index = binder.index();
            let patterns = index
                .path_template_for(&record.product)
                .filter(|template| !template.writes_paths())
                .zip(index.product(&record.product))
                .map(|(template, product)| {
                    template
                        .rules()
                        .map(|rule| PathPattern::new(rule, &product.name, None))
                        .collect()
                });
            readers.insert(record.product.clone(), patterns);
        }
        if let Some(patterns) = &readers[&record.product] {
            read_back(record, patterns)?;
            continue;
        }
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
        let path = binder.bind(artifact.view(), || format!("source `{artifact}`"))?;
        if let Some(given) = &record.path {
            if *given != path {
                return Err(error(format!(
                    "source `{artifact}` names `{given}`, but its path rule gives `{path}`; a record names its file only when its source's rules write no path, so remove `: {given}`"
                )));
            }
        }
        record.path = Some(path);
    }
    Ok(())
}

fn read_back(record: &SourceRecord, patterns: &[PathPattern]) -> Result<(), PathError> {
    let shown = || identity(&record.product, record.entities.iter());
    let Some(path) = &record.path else {
        return Err(error(format!(
            "source `{}` names no file; its path rule has `*`, a fixed value or several lines, so it gives no path, and each record names its file, as `{}: <path>`",
            shown(),
            shown()
        )));
    };
    let reads = patterns.iter().any(|pattern| {
        pattern.match_path(path).is_some_and(|bound| {
            read_binding(path, bound, pattern.fixed()).is_ok_and(|read| read == record.entities)
        })
    });
    if reads {
        return Ok(());
    }
    let rules: Vec<_> = patterns
        .iter()
        .map(|pattern| format!("`{pattern}`"))
        .collect();
    Err(error(format!(
        "source `{}` names `{path}`, which its path rule {} does not read as it",
        shown(),
        rules.join(" or ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_gone_when_it_is_read_is_absent_but_a_missing_root_is_an_error() {
        let root = std::env::temp_dir().join(format!("spit-gone-{}", std::process::id()));
        fs::create_dir_all(&root).expect("a temporary folder can be made");
        let mut listing = Listing::default();
        let walk = Walk {
            named_hidden: &BTreeSet::new(),
            outputs: None,
        };
        let gone = root.join("gone");
        let read = listing.walk(&gone, None, "gone/", &mut BTreeSet::new(), &walk);
        assert!(matches!(read, Ok(false)));
        assert!(listing.directories.is_empty() && listing.files.is_empty());
        assert!(listing
            .walk(&gone, None, "", &mut BTreeSet::new(), &walk)
            .is_err());
        fs::remove_dir_all(&root).expect("the temporary folder can be removed");
    }
}
