//! The input stage: settle which contexts and sources a dataset holds.
//!
//! It reads a `.spitin` recipe of `discover`, `exclude`, `require` and source
//! path rules, and the pipeline's source declarations. It scans a root or
//! takes records already written, and returns a plain inventory with what it
//! removed and what the `require` rules find missing. Resolving jobs needs
//! nothing else from it.

mod coverage;
mod discover;
mod exclusions;
mod pattern;
mod suggest;
mod variants;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::compile::validate_pipeline;
use crate::error::ResolveError;
use crate::imports::parse_located_document;
use crate::lower::{parse_document_with_imports, ParsedDocument};
use crate::model::{
    ArtifactInstance, CoverageAction, CoverageGap, EntityBinding, GroupKey, InputRules, Pipeline,
    PipelineIndex, Removal, SourceInventory,
};
use crate::parser::{strip_comment, without_bom, Header, Keyword, Kind, ParseError, SourceMap};
use crate::paths::{inspect_paths, PathError, PathTemplate};

pub(crate) use self::coverage::collect_rule_errors;
pub use self::coverage::EveryGroupDropped;
use self::coverage::{apply_drops, check_before_removal};
pub(crate) use self::coverage::{check_inventory, InputCheck};
pub(crate) use self::discover::with_source_paths;
use self::discover::{discover, locate_sources, unmatched_files};
pub use self::discover::{discover_source_files, discover_sources, Discovery};
pub(crate) use self::exclusions::collect_exclusion_errors;
pub use self::exclusions::UnmatchedExclusion;
use self::exclusions::{read_exclusion_files, Excluder};
pub use self::pattern::{MissedSource, NearestFile};
pub use self::suggest::{NearlyMatched, SuggestedSource, Suggestions};
pub use self::variants::{case_variants, CaseVariants, Spelling};

/// A recipe's rules and any inventory records written with them.
#[derive(Clone, Debug, Default)]
pub struct InputSpec {
    /// The pipeline a recipe's `pipeline analysis.spit` line names: relative
    /// to the recipe's folder when parsed at a path.
    pub pipeline: Option<PathBuf>,
    /// The dataset root a recipe's `root data` line names, relative to the
    /// recipe's folder when parsed at a path, and the line it is on.
    pub root: Option<(PathBuf, usize)>,
    pub rules: InputRules,
    pub inventory: Option<SourceInventory>,
}

/// Parse a `.spitin` file without resolving imports.
pub fn parse_input_spec(text: &str) -> Result<InputSpec, ParseError> {
    let (header, document) = parse_recipe(text, |text| {
        parse_document_with_imports(text, &BTreeMap::new(), Kind::Recipe)
    })?;
    // Files an `exclude from` line names are read from the working folder.
    finish_spec(document, header, Some(Path::new("")))
}

/// Parse a `.spitin` file at `path`. The pipeline it names and its root are
/// relative to its folder, and a recipe that names its pipeline must name
/// its root: a file says where its data is, whoever runs it.
pub fn parse_input_spec_at(text: &str, path: &Path) -> Result<InputSpec, ParseError> {
    let (header, document) = parse_recipe(text, |text| {
        parse_located_document(text, path, Kind::Recipe)
    })?;
    if header.pipeline.is_some() && header.root.is_none() {
        return Err(ParseError::new(
            1,
            "a recipe names its dataset root, the folder its paths are relative to, with a line such as `root data`, or `root .` for the recipe's own folder",
        ));
    }
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    let header = RecipeHeader {
        pipeline: header.pipeline.map(|pipeline| folder.join(pipeline)),
        root: header.root.map(|(root, line)| (folder.join(root), line)),
    };
    finish_spec(document, header, Some(folder))
}

/// Parse a `.spitin` without resolving imports, with where each of its
/// rules is written, for diagnostics.
pub(crate) fn parse_recipe_lines(text: &str) -> Result<(InputSpec, SourceMap), ParseError> {
    let (header, mut document) = parse_recipe(text, |text| {
        parse_document_with_imports(text, &BTreeMap::new(), Kind::Recipe)
    })?;
    let lines = std::mem::take(&mut document.lines);
    // Its `exclude from` files are left unread: its rules' places are the
    // lines of the recipe, and a file's rows have none there.
    Ok((finish_spec(document, header, None)?, lines))
}

/// The folders a recipe's `pipeline` and `root` lines name, as written.
#[derive(Default)]
struct RecipeHeader {
    pipeline: Option<PathBuf>,
    root: Option<(PathBuf, usize)>,
}

/// A recipe's `pipeline` and `root` lines, and the document its other
/// lines parse to with `parse` once each is checked to belong in a recipe.
fn parse_recipe(
    text: &str,
    parse: impl FnOnce(&str) -> Result<ParsedDocument, ParseError>,
) -> Result<(RecipeHeader, ParsedDocument), ParseError> {
    let (header, text) = header_lines(without_bom(text))?;
    check_input_lines(&text)?;
    Ok((header, parse(&text)?))
}

/// The pipeline a recipe names with `pipeline analysis.spit` and the root
/// it names with `root data`, and the text with those lines blanked so that
/// other lines keep their numbers.
fn header_lines(text: &str) -> Result<(RecipeHeader, String), ParseError> {
    let mut header = RecipeHeader::default();
    let mut rest = String::new();
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
        if let Some(file) = line.strip_prefix("pipeline ") {
            if header.pipeline.is_some() {
                return Err(ParseError::new(number, "a .spitin names its pipeline once"));
            }
            header.pipeline = Some(PathBuf::from(file.trim()));
        } else if let Some(folder) = line.strip_prefix("root ") {
            if header.root.is_some() {
                return Err(ParseError::new(number, "a .spitin names its root once"));
            }
            header.root = Some((PathBuf::from(folder.trim()), number));
        } else {
            rest.push_str(original);
        }
        rest.push('\n');
    }
    Ok((header, rest))
}

/// Why a line belongs in the pipeline rather than the recipe: what each
/// file holds.
const PIPELINE_ONLY: &str = ", which every dataset shares; a .spitin binds it to one dataset with \
     its `root`, source paths, and `discover`, `exclude` and `require` rules";

/// Whether a line that starts with no keyword is a step, `out = f(in)`,
/// rather than a record, whose `[` comes before any `=`.
fn is_step(line: &str) -> bool {
    Keyword::of(line).is_none()
        && line
            .split_once('=')
            .is_some_and(|(outputs, call)| !outputs.contains('[') && call.contains('('))
}

fn check_input_lines(text: &str) -> Result<(), ParseError> {
    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        let pipeline_only = matches!(
            Keyword::of(line),
            Some(
                Keyword::Source
                    | Keyword::Operation
                    | Keyword::Command
                    | Keyword::Verify
                    | Keyword::Check
                    | Keyword::Checks
                    | Keyword::Stage
                    | Keyword::Dimensions
                    | Keyword::Sidecars
                    | Keyword::Use
            )
        );
        if pipeline_only {
            let word = line.split_once([' ', ':']).map_or(line, |(word, _)| word);
            return Err(ParseError::new(
                index + 1,
                format!("`{word}` belongs in the .spit pipeline{PIPELINE_ONLY}"),
            ));
        }
        if is_step(line) {
            return Err(ParseError::new(
                index + 1,
                format!("a step belongs in the .spit pipeline{PIPELINE_ONLY}"),
            ));
        }
        if Keyword::of(line) == Some(Keyword::Ext) {
            return Err(ParseError::new(
                index + 1,
                "`ext:` completes the pipeline's default output paths; it belongs in the .spit pipeline",
            ));
        }
        if matches!(Header::of(line), Some(Header::SourcePaths)) {
            return Err(ParseError::new(
                index + 1,
                "`source_paths:` belongs in a .spitout; use `path image:` in a .spitin",
            ));
        }
    }
    Ok(())
}

/// A recipe holds rules and records, and paths only for sources; it names
/// `pipeline`. Its `path:` line is the default for sources.
fn finish_spec(
    document: ParsedDocument,
    header: RecipeHeader,
    folder: Option<&Path>,
) -> Result<InputSpec, ParseError> {
    let ParsedDocument {
        pipeline,
        mut inputs,
        inventory,
        lines,
    } = document;
    if !pipeline.products.is_empty()
        || !pipeline.operations.is_empty()
        || !pipeline.invocations.is_empty()
        || !pipeline.commands.is_empty()
        || !pipeline.stages.is_empty()
    {
        return Err(ParseError::new(1, "a .spitin file may contain discovery, coverage, source paths, and inventory records only"));
    }
    if let Some(default) = &pipeline.path_template {
        if default.needs_stage() {
            let line = lines.default_path.as_ref().map_or(1, |place| place.line);
            return Err(ParseError::new(
                line,
                "a .spitin `path:` is the default for sources, and no source is made in a stage; leave out `{@stage}`",
            ));
        }
    }
    inputs.source_paths = pipeline.product_paths;
    inputs.source_default = pipeline.path_template;
    if let Some(folder) = folder {
        read_exclusion_files(&mut inputs, folder)?;
    }
    Ok(InputSpec {
        pipeline: header.pipeline,
        root: header.root,
        rules: inputs,
        inventory,
    })
}

impl InputSpec {
    /// Check the recipe against the pipeline's source declarations, without
    /// reading any file or record.
    pub fn check(&self, pipeline: &Pipeline) -> Result<(), InputError> {
        let index = PipelineIndex::new(pipeline);
        for name in self.rules.source_paths.keys() {
            let product = name.clone();
            if !index.is_source(name) {
                if index.product(name).is_some() {
                    return Err(InputError::OutputPath { product });
                }
                return Err(InputError::NotASource { product });
            }
            if let Some(group) = index
                .product(name)
                .and_then(|source| source.beside.as_ref())
            {
                let group = group.sibling.clone();
                return Err(InputError::MemberPath { product, group });
            }
            if pipeline.product_paths.contains_key(name) {
                return Err(InputError::PathInBoth { product });
            }
        }
        // Directory discovery finds every source's files.
        if !self.rules.discoveries.is_empty() {
            if let Some(product) = self.source_without_path(pipeline) {
                return Err(InputError::NoSourcePath { product });
            }
        }
        if let Some((_, error)) = collect_rule_errors(pipeline, &self.rules, &BTreeSet::new())
            .into_iter()
            .next()
        {
            return Err(error.into());
        }
        Ok(())
    }

    /// The first source of `pipeline` that no rule covers: not its own in
    /// either file, nor the recipe's default, nor the pipeline's. A scan
    /// finds every source by its rule, so it cannot find this one.
    pub fn source_without_path(&self, pipeline: &Pipeline) -> Option<String> {
        let source_paths = self.rules.source_paths_for(pipeline);
        let located = with_source_paths(pipeline, &source_paths);
        let index = PipelineIndex::new(&located);
        pipeline
            .products
            .iter()
            .find(|product| index.is_source(&product.name) && !index.has_path(&product.name))
            .map(|product| product.name.clone())
    }

    /// Run the input stage: find the contexts and source files a dataset
    /// holds, apply the `exclude` rules, check the `require` rules, and give
    /// each source record its file's path.
    ///
    /// The stage reads `pipeline` only for its source products and leaves it
    /// untouched. What it returns is a plain inventory, so resolving jobs
    /// never sees a discovery, exclude, drop or require rule.
    pub fn resolve(
        &self,
        pipeline: &Pipeline,
        source: InputSource<'_>,
    ) -> Result<ResolvedInputs, InputError> {
        validate_pipeline(pipeline)?;
        self.check(pipeline)?;
        let source_paths = self.rules.source_paths_for(pipeline);
        let (mut inventory, skipped, unmatched_files, missed_sources, root, removed, incomplete) =
            match source {
                InputSource::Discover(root) => {
                    if let Some(product) = self.source_without_path(pipeline) {
                        return Err(InputError::NoSourcePath { product });
                    }
                    let located = with_source_paths(pipeline, &source_paths);
                    let found = discover(&located, &self.rules, root)?;
                    if let Some(unmatched) = found.unmatched {
                        return Err(InputError::UnmatchedExclusion(unmatched));
                    }
                    if let Some(emptied) = found.emptied {
                        return Err(InputError::EveryGroupDropped(emptied));
                    }
                    let incomplete = incomplete_groups(pipeline, &found.inventory, &found.removed);
                    (
                        found.inventory,
                        found.skipped,
                        found.unmatched_files,
                        found.missed,
                        Some(root.to_owned()),
                        found.removed,
                        incomplete,
                    )
                }
                InputSource::Inventory(mut inventory) => {
                    let removes = !self.rules.exclusions.is_empty()
                        || self
                            .rules
                            .constraints
                            .iter()
                            .any(|rule| rule.action == CoverageAction::Drop);
                    if removes {
                        check_before_removal(pipeline, &self.rules, &inventory)?;
                    }
                    let mut excluder = Excluder::new(&self.rules.exclusions);
                    excluder.apply(&mut inventory);
                    let mut removed = excluder.finish().map_err(InputError::UnmatchedExclusion)?;
                    let dropped = apply_drops(&self.rules, &mut inventory)
                        .map_err(InputError::EveryGroupDropped)?;
                    removed.extend(dropped.iter().map(|group| group.removal()));
                    let incomplete = incomplete_groups(pipeline, &inventory, &removed);
                    (
                        inventory,
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        None,
                        removed,
                        incomplete,
                    )
                }
            };
        merge_source_paths(pipeline, &source_paths, &mut inventory)?;
        let located = with_source_paths(pipeline, &inventory.source_paths);
        inspect_paths(&located)?;
        let checked = check_inventory(pipeline, &self.rules, Cow::Owned(inventory))?;
        let mut inventory = checked.inventory.into_owned();
        // After any record the inventory already held, as a .spitout does.
        inventory.removed.extend(removed);
        locate_sources(&located, &mut inventory)?;
        Ok(ResolvedInputs {
            inventory,
            skipped,
            unmatched_files,
            missed_sources,
            incomplete_groups: incomplete,
            gaps: checked.gaps,
            root,
        })
    }

    /// Suggest a source and path rule for each group of files under `root`
    /// that no source rule matches, by the recipe's rules and `pipeline`'s.
    /// A source with no rule is not an error here: a group of files may be
    /// suggested as that source.
    pub fn suggest(&self, pipeline: &Pipeline, root: &Path) -> Result<Suggestions, InputError> {
        let located = with_source_paths(pipeline, &self.rules.source_paths_for(pipeline));
        let (files, declared) = unmatched_files(&located, root)?;
        let taken = pipeline
            .products
            .iter()
            .map(|product| product.name.as_str())
            .filter(|name| !declared.iter().any(|product| product.name == *name))
            .collect();
        Ok(suggest::suggest(&files, &declared, &taken))
    }

    /// Give a pipeline the recipe's source paths, so an editor can check
    /// every path rule of the two files together. Resolving jobs does not
    /// need them: the input stage writes each source's path into its record.
    pub fn apply_paths(&self, pipeline: &mut Pipeline) {
        let source_paths = self.rules.source_paths_for(pipeline).into_owned();
        pipeline.product_paths.extend(source_paths);
    }
}

/// Add `source_paths`, the recipe's rule for each source, to those
/// `inventory` carries, as a `.spitout` does, so the inventory holds every
/// rule for its sources. Each must name a source the pipeline declares no
/// rule for, and the two files must not disagree.
fn merge_source_paths(
    pipeline: &Pipeline,
    source_paths: &BTreeMap<String, PathTemplate>,
    inventory: &mut SourceInventory,
) -> Result<(), InputError> {
    for (name, template) in source_paths {
        if inventory
            .source_paths
            .get(name)
            .is_some_and(|existing| existing != template)
        {
            return Err(InputError::ConflictingSourcePaths {
                product: name.clone(),
            });
        }
        inventory
            .source_paths
            .insert(name.clone(), template.clone());
    }
    let index = PipelineIndex::new(pipeline);
    for name in inventory.source_paths.keys() {
        let product = name.clone();
        if !index.is_source(name) {
            return Err(InputError::UnknownSourcePath { product });
        }
        if let Some(beside) = index
            .product(name)
            .and_then(|source| source.beside.as_ref())
        {
            return Err(InputError::MemberPath {
                product,
                group: beside.sibling.clone(),
            });
        }
        if pipeline.product_paths.contains_key(name) {
            return Err(InputError::InventoryPathInBoth { product });
        }
    }
    Ok(())
}

/// Each binding where some of a main source's companions were found and
/// others were neither found nor removed, by `removed` or a removal
/// `inventory` already records from an earlier run, said as
/// `photo[site=A,visit=2,shot=3] has .raw and .gpx but no .imu`, by group
/// and then by value.
///
/// Records are grouped by the symbols they bind, in one pass, and only an
/// incomplete group's binding is written as text.
fn incomplete_groups(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    removed: &[Removal],
) -> Vec<String> {
    let groups = &pipeline.sidecar_groups;
    if groups.is_empty() {
        return Vec::new();
    }
    // Each member source's group and its place among the group's members.
    let member_of: FxHashMap<&str, (usize, usize)> = groups
        .iter()
        .enumerate()
        .flat_map(|(group, sidecars)| {
            sidecars
                .members
                .iter()
                .enumerate()
                .map(move |(member, (name, _))| (name.as_str(), (group, member)))
        })
        .collect();
    // For each group, the members found at each binding of its dimensions,
    // with a record's binding to name the place by.
    type Found<'a> = FxHashMap<GroupKey, (&'a EntityBinding, Vec<bool>)>;
    let mut found: Vec<Found<'_>> = vec![FxHashMap::default(); groups.len()];
    for record in &inventory.artifacts {
        let Some(&(group, member)) = member_of.get(record.product.as_str()) else {
            continue;
        };
        let sidecars = &groups[group];
        let Some(key) = record.entities.group_key(&sidecars.dimensions) else {
            continue;
        };
        found[group]
            .entry(key)
            .or_insert_with(|| (&record.entities, vec![false; sidecars.members.len()]))
            .1[member] = true;
    }
    let mut said = Vec::new();
    for (sidecars, found) in groups.iter().zip(found) {
        let mut incomplete: Vec<_> = found
            .into_values()
            .filter(|(_, present)| present.contains(&false))
            .collect();
        incomplete
            .sort_unstable_by(|(left, _), (right, _)| left.cmp_in(right, &sidecars.dimensions));
        for (binding, present) in incomplete {
            let removed_here = |member: &str| {
                inventory.removed.iter().chain(removed).any(|removal| {
                    removal
                        .product
                        .as_deref()
                        .is_none_or(|product| product == member)
                        && removal.entities.within(binding, &sidecars.dimensions)
                })
            };
            let (has, lacks): (Vec<_>, Vec<_>) = sidecars
                .members
                .iter()
                .zip(&present)
                .filter(|&((member, _), &here)| here || !removed_here(member))
                .partition(|&(_, &here)| here);
            if !lacks.is_empty() {
                let extensions = |members: Vec<(&(String, String), &bool)>| {
                    listed(
                        members
                            .into_iter()
                            .map(|((_, extension), _)| extension.as_str()),
                    )
                };
                let values = sidecars
                    .dimensions
                    .iter()
                    .map(|dimension| {
                        let value = binding
                            .get(dimension)
                            .expect("a binding with a key binds its dimensions");
                        format!("{dimension}={value}")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                said.push(format!(
                    "{}[{values}] has {} but no {}",
                    sidecars.name,
                    extensions(has),
                    extensions(lacks)
                ));
            }
        }
    }
    said
}

/// `a`, `a and b`, or `a, b and c`.
fn listed<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let items: Vec<_> = items.collect();
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => (*last).to_owned(),
        None => String::new(),
    }
}

/// Why a recipe cannot be applied to a pipeline, or to a dataset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputError {
    /// The pipeline does not compile, a rule does not fit it, or the
    /// records break a rule.
    Resolve(ResolveError),
    /// A path rule is invalid, or a file under the root is missing or
    /// cannot be read.
    Path(PathError),
    /// The recipe sets a path for a product the pipeline does not declare.
    NotASource { product: String },
    /// The recipe sets a path for a product a step makes, whose path is
    /// the pipeline's to give.
    OutputPath { product: String },
    /// A source has a path rule in both the pipeline
    /// and the recipe.
    PathInBoth { product: String },
    /// The recipe sets a path for a companion, which follows its main source.
    MemberPath { product: String, group: String },
    /// A scan finds every source's files, and this one has no path rule to
    /// find them by.
    NoSourcePath { product: String },
    /// The recipe and the `.spitout` give a source different path rules.
    ConflictingSourcePaths { product: String },
    /// A `.spitout` gives a path rule for a product that is not a source.
    UnknownSourcePath { product: String },
    /// A source has a path rule in both the pipeline and the `.spitout`.
    InventoryPathInBoth { product: String },
    /// An `exclude` rule matches nothing in the dataset.
    UnmatchedExclusion(UnmatchedExclusion),
    /// conditional `exclude` rules remove every group of a grouping.
    EveryGroupDropped(EveryGroupDropped),
}

impl From<ResolveError> for InputError {
    fn from(error: ResolveError) -> Self {
        Self::Resolve(error)
    }
}

impl From<PathError> for InputError {
    fn from(error: PathError) -> Self {
        Self::Path(error)
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolve(error) => error.fmt(f),
            Self::Path(error) => error.fmt(f),
            Self::NotASource { product } => write!(
                f,
                "input path `{product}` must name a source product in the pipeline"
            ),
            Self::OutputPath { product } => write!(
                f,
                "`{product}` is made by a step, so its path belongs in the .spit pipeline; a .spitin gives paths only for sources"
            ),
            Self::PathInBoth { product } => write!(
                f,
                "`{product}` has path rules in both .spit and .spitin; keep the pipeline's if every dataset has this layout, or the recipe's if only this one does"
            ),
            Self::MemberPath { product, group } => write!(
                f,
                "source `{product}` is beside `{group}` and takes its path from it; write `path {group}:` instead"
            ),
            Self::NoSourcePath { product } => write!(
                f,
                "source `{product}` has no path rule, so its files cannot be found; write `path {product}:` in the pipeline or the recipe, or a default `path:` in the recipe"
            ),
            Self::ConflictingSourcePaths { product } => write!(
                f,
                "source `{product}` has conflicting path rules in .spitin and .spitout"
            ),
            Self::UnknownSourcePath { product } => {
                write!(f, "source path rule names unknown source `{product}`")
            }
            Self::InventoryPathInBoth { product } => write!(
                f,
                "source `{product}` has path rules in both .spit and .spitout"
            ),
            Self::EveryGroupDropped(emptied) => emptied.fmt(f),
            Self::UnmatchedExclusion(unmatched) => {
                write!(
                    f,
                    "`{}` ({}) matches nothing in this dataset",
                    unmatched.rule, unmatched.origin
                )?;
                if !unmatched.near.is_empty() {
                    write!(f, "; it has {}", unmatched.near.join(", "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for InputError {}

/// Where the input stage gets its inventory.
pub enum InputSource<'a> {
    /// Scan the directory the recipe's rules describe.
    Discover(&'a Path),
    /// Use records already written, from a `.spitout` or the recipe itself.
    Inventory(SourceInventory),
}

/// What the input stage settled about a dataset.
#[derive(Debug)]
pub struct ResolvedInputs {
    /// The contexts and sources that remain after `exclude` rules, with the named
    /// discovery contexts kept for the `.spitout`.
    pub inventory: SourceInventory,
    /// Each file left out because a value in its path cannot be read, and
    /// why. What the recipe's rules removed is in `inventory.removed`.
    pub skipped: Vec<String>,
    /// Files under a scanned root that matched no source path rule.
    pub unmatched_files: Vec<String>,
    /// Each source whose path rule matched no file under a scanned root.
    pub missed_sources: Vec<MissedSource>,
    /// Each place the scan or records hold some of a companion group's
    /// sources and not the others, as `photo[site=A,shot=3] has .raw and
    /// .gpx but no .imu`.
    pub incomplete_groups: Vec<String>,
    /// What the `require` rules find missing, with the sources each holds back.
    pub gaps: Vec<CoverageGap>,
    /// The directory that was scanned, when the stage scanned one.
    pub root: Option<PathBuf>,
}

impl ResolvedInputs {
    /// The first missing requirement, as an error.
    pub fn require_complete(&self) -> Result<(), ResolveError> {
        self.gaps
            .first()
            .map_or(Ok(()), |gap| Err(gap.error.clone()))
    }

    /// The sources a missing requirement holds back, which no job can use.
    pub fn unavailable(&self) -> Vec<ArtifactInstance> {
        self.gaps
            .iter()
            .flat_map(|gap| gap.sources.iter().cloned())
            .collect()
    }

    /// The inventory for resolving jobs: the same contexts and sources without
    /// the bookkeeping that ties them to discovery rules.
    pub fn dag_inventory(&self) -> SourceInventory {
        SourceInventory {
            discovered: BTreeMap::new(),
            ..self.inventory.clone()
        }
    }
}
