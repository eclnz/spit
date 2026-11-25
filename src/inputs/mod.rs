mod coverage;
mod discover;
mod duplicates;
mod error;
mod exclusions;
mod local;
mod pattern;
mod recipe;
mod reviews;
mod tools;
mod variants;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::compile::validate_pipeline;
use crate::lower::imports::parse_located_document;
use crate::lower::{parse_document_with_imports, ParsedDocument};
use crate::model::ResolveError;
use crate::model::{
    ArtifactInstance, CoverageAction, CoverageGap, DefaultChecks, DefaultReviews, EntityBinding,
    GroupKey, InputRules, Pipeline, PipelineIndex, Removal, ReviewRecord, SourceInventory,
    TypeChecking,
};
use crate::parser::error::RecipeError;
use crate::parser::{
    without_bom, File, Head, Header, Keyword, Kind, Layout, ParseError, SourceMap,
};
use crate::paths::inspect_paths;
use crate::template::PathTemplate;

pub(crate) use self::coverage::collect_rule_errors;
pub use self::coverage::EveryGroupDropped;
use self::coverage::{apply_drops, check_before_removal};
pub(crate) use self::coverage::{check_inventory, InputCheck};
pub(crate) use self::discover::with_source_paths;
use self::discover::{discover, locate_sources};
pub use self::discover::{discover_source_files, discover_sources, Discovery};
pub use self::error::InputError;
use self::exclusions::Excluder;
pub use self::exclusions::UnmatchedExclusion;
pub use self::pattern::{MissedSource, NearestFile};
use self::recipe::{expand_input_block, header_lines, restore_path_places, RecipeHeader};
pub use self::reviews::ReviewFileError;
use self::reviews::{read_review_files, read_verdicts, without_review_files};
pub(crate) use self::tools::missing_tool;
use self::tools::read_tools_file;
pub use self::tools::{parse_tools, ToolsFile};
pub use self::variants::{variants, Likeness, Spelling, Variants};

#[derive(Clone, Debug, Default)]
pub struct InputSpec {
    pub pipeline: Option<PathBuf>,
    pub root: Option<(PathBuf, usize)>,
    pub output: Option<(PathBuf, usize)>,
    pub tools: Option<ToolsFile>,
    pub rules: InputRules,
    pub inventory: Option<SourceInventory>,
}

pub fn parse_input_spec(text: &str) -> Result<InputSpec, ParseError> {
    let (header, document) = parse_recipe(text, |text| {
        parse_document_with_imports(text, &BTreeMap::new(), Kind::Recipe)
    })?;
    finish_spec(document, header, Some(Path::new("")), true)
}

pub fn parse_input_spec_at(text: &str, path: &Path) -> Result<InputSpec, ParseError> {
    parse_spec_at(text, path, true)
}

pub fn parse_input_spec_for_dag(text: &str, path: &Path) -> Result<InputSpec, ParseError> {
    parse_spec_at(text, path, false)
}

fn parse_spec_at(text: &str, path: &Path, read_reviews: bool) -> Result<InputSpec, ParseError> {
    let (header, document) = parse_recipe(text, |text| {
        parse_located_document(text, path, Kind::Recipe)
    })?;
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    let header = RecipeHeader {
        pipeline: header.pipeline.map(|pipeline| folder.join(pipeline)),
        root: header.root.map(|(root, line)| (folder.join(root), line)),
        output: header
            .output
            .map(|(output, line)| (folder.join(output), line)),
        tools: header.tools,
    };
    finish_spec(document, header, Some(folder), read_reviews)
}

pub(crate) fn parse_recipe_lines(text: &str) -> Result<(InputSpec, SourceMap), ParseError> {
    let (header, mut document) = parse_recipe(text, |text| {
        parse_document_with_imports(text, &BTreeMap::new(), Kind::Recipe)
    })?;
    let lines = std::mem::take(&mut document.lines);
    Ok((finish_spec(document, header, None, false)?, lines))
}

fn parse_recipe(
    text: &str,
    parse: impl FnOnce(&str) -> Result<ParsedDocument, ParseError>,
) -> Result<(RecipeHeader, ParsedDocument), ParseError> {
    let (header, original) = header_lines(without_bom(text))?;
    let (text, contexts) = expand_input_block(&original)?;
    check_input_lines(&text)?;
    let mut document = parse(&text)?;
    restore_path_places(&original, &mut document.lines);
    document.inputs.context_paths = contexts;
    Ok((header, document))
}

fn pipeline_only(keyword: Keyword) -> bool {
    keyword.spec().file == File::Pipeline
}

fn check_input_lines(text: &str) -> Result<(), ParseError> {
    for (number, line) in Layout::new(text).numbered() {
        let (keyword, written) = match line.head {
            Head::Keyword(Keyword::Default, rest) => (
                Keyword::split(rest.trim()).map(|(keyword, _)| keyword),
                line.code,
            ),
            _ => (
                line.keyword(),
                line.code
                    .split_once([' ', ':'])
                    .map_or(line.code, |(word, _)| word),
            ),
        };
        if keyword == Some(Keyword::Ext) {
            return Err(ParseError::of(number, RecipeError::DefaultExtInRecipe));
        }
        if keyword.is_some_and(pipeline_only) {
            let word = written.split_once(':').map_or(written, |(word, _)| word);
            return Err(ParseError::of(
                number,
                RecipeError::BelongsInPipeline {
                    word: word.to_string(),
                },
            ));
        }
        if line.head == Head::Step {
            return Err(ParseError::of(number, RecipeError::StepInRecipe));
        }
        if line.header() == Some(Header::SourcePaths) {
            return Err(ParseError::of(number, RecipeError::SourcePathsInRecipe));
        }
    }
    Ok(())
}

fn finish_spec(
    document: ParsedDocument,
    header: RecipeHeader,
    folder: Option<&Path>,
    read_reviews: bool,
) -> Result<InputSpec, ParseError> {
    let ParsedDocument {
        pipeline,
        mut inputs,
        inventory,
        lines,
    } = document;
    let Pipeline {
        product_paths,
        path_template,
        entities_format,
        files: _,
        folder: _,
        root,
        output,
        products,
        operations,
        invocations,
        commands,
        resources,
        tools,
        uses,
        default_uses,
        checks,
        default_checks,
        reviews,
        default_reviews,
        extension,
        stages,
        sidecar_groups,
        calls,
        types,
        type_decls,
    } = pipeline;
    if root.is_some()
        || output.is_some()
        || !products.is_empty()
        || !operations.is_empty()
        || !invocations.is_empty()
        || !commands.is_empty()
        || !resources.is_empty()
        || !tools.is_empty()
        || !uses.is_empty()
        || default_uses.is_some()
        || !checks.is_empty()
        || default_checks != DefaultChecks::default()
        || !reviews.is_empty()
        || default_reviews != DefaultReviews::default()
        || extension.is_some()
        || !stages.is_empty()
        || !sidecar_groups.is_empty()
        || !calls.is_empty()
        || types != TypeChecking::default()
        || !type_decls.is_empty()
    {
        return Err(ParseError::of(1, RecipeError::RecipeHoldsOtherContent));
    }
    if let Some(default) = &path_template {
        if default.needs_stage() {
            let line = lines.default_path.as_ref().map_or(1, |place| place.line);
            return Err(ParseError::of(line, RecipeError::StageInInputsDefault));
        }
    }
    inputs.source_paths = product_paths;
    inputs.source_default = path_template;
    inputs.source_entities = entities_format;
    let mut tools = None;
    if let Some(folder) = folder {
        if read_reviews {
            read_review_files(&mut inputs, folder)?;
        } else {
            for review in &mut inputs.reviews {
                review.path = folder.join(&review.file);
            }
        }
        if let Some((file, line)) = &header.tools {
            tools = Some(read_tools_file(folder, file, *line)?);
        }
    }
    Ok(InputSpec {
        pipeline: header.pipeline,
        root: header.root,
        output: header.output,
        tools,
        rules: inputs,
        inventory,
    })
}

impl InputSpec {
    pub fn root_for<'a>(
        &'a self,
        pipeline: &'a Pipeline,
    ) -> Result<Option<&'a (PathBuf, usize)>, InputError> {
        either(&self.root, &pipeline.root, InputError::RootInBoth)
    }

    pub fn output_for<'a>(
        &'a self,
        pipeline: &'a Pipeline,
    ) -> Result<Option<&'a (PathBuf, usize)>, InputError> {
        either(&self.output, &pipeline.output, InputError::OutputInBoth)
    }

    pub fn check(&self, pipeline: &Pipeline) -> Result<(), InputError> {
        self.root_for(pipeline)?;
        self.output_for(pipeline)?;
        if let Some(tools) = &self.tools {
            if let Some(tool) = missing_tool(pipeline, tools) {
                return Err(InputError::MissingTool {
                    tool: tool.to_owned(),
                    file: tools.file.clone(),
                    line: tools.line,
                });
            }
        }
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
        if let Some(format) = &self.rules.source_entities {
            let dimensions = pipeline.dimension_order();
            if let Some(dimension) = format.labels.keys().find(|name| !dimensions.contains(name)) {
                return Err(InputError::Path(crate::template::error(format!(
                    "the recipe's entity label names unknown dimension `{dimension}`"
                ))));
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
                    let outputs = self
                        .output_for(pipeline)?
                        .map(|(folder, _)| folder.as_path());
                    let found = discover(&located, &self.rules, root, outputs)?;
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
        let unmatched_files = without_review_files(unmatched_files, root.as_deref(), &self.rules);
        merge_source_paths(pipeline, &source_paths, &mut inventory)?;
        inventory
            .verdicts
            .extend(read_verdicts(pipeline, &self.rules).map_err(InputError::ReviewFile)?);
        inventory
            .review_files
            .extend(self.rules.reviews.iter().map(|review| ReviewRecord {
                review: review.review.clone(),
                file: review.path.clone(),
                found: review.text.is_some(),
            }));
        let located = with_source_paths(pipeline, &inventory.source_paths);
        inspect_paths(&located)?;
        let checked = check_inventory(pipeline, &self.rules, Cow::Owned(inventory))?;
        let mut inventory = checked.inventory.into_owned();
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

    pub fn apply_paths(&self, pipeline: &mut Pipeline) {
        let source_paths = self.rules.source_paths_for(pipeline).into_owned();
        pipeline.product_paths.extend(source_paths);
    }
}

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

fn incomplete_groups(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    removed: &[Removal],
) -> Vec<String> {
    let groups = &pipeline.sidecar_groups;
    if groups.is_empty() {
        return Vec::new();
    }
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

fn either<'a>(
    recipe: &'a Option<(PathBuf, usize)>,
    pipeline: &'a Option<(PathBuf, usize)>,
    both: InputError,
) -> Result<Option<&'a (PathBuf, usize)>, InputError> {
    match (recipe, pipeline) {
        (Some(_), Some(_)) => Err(both),
        (recipe, pipeline) => Ok(recipe.as_ref().or(pipeline.as_ref())),
    }
}

fn listed<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let items: Vec<_> = items.collect();
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => (*last).to_owned(),
        None => String::new(),
    }
}

pub enum InputSource<'a> {
    Discover(&'a Path),
    Inventory(SourceInventory),
}

#[derive(Debug)]
pub struct ResolvedInputs {
    pub inventory: SourceInventory,
    pub skipped: Vec<String>,
    pub unmatched_files: Vec<String>,
    pub missed_sources: Vec<MissedSource>,
    pub incomplete_groups: Vec<String>,
    pub gaps: Vec<CoverageGap>,
    pub root: Option<PathBuf>,
}

impl ResolvedInputs {
    pub fn require_complete(&self) -> Result<(), ResolveError> {
        self.gaps
            .first()
            .map_or(Ok(()), |gap| Err(gap.error.clone()))
    }

    pub fn unavailable(&self) -> Vec<ArtifactInstance> {
        self.gaps
            .iter()
            .flat_map(|gap| gap.sources.iter().cloned())
            .collect()
    }

    pub fn dag_inventory(&self) -> SourceInventory {
        self.inventory.clone()
    }
}
