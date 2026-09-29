//! The input stage: settle which contexts and sources a dataset holds.
//!
//! It reads a `.spitin` recipe of `discover`, `require`, `skip` and source
//! path rules, and the pipeline's source declarations. It scans a root or
//! takes records already written, and returns a plain inventory with what it
//! skipped and what the `require` rules find missing. Resolving jobs needs
//! nothing else from it.

mod coverage;
mod discover;

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::compile::validate_pipeline;
use crate::error::ResolveError;
use crate::imports::parse_located_document;
use crate::lower::{parse_document_with_imports, ParsedDocument};
use crate::model::{ArtifactInstance, CoverageGap, InputRules, Pipeline, SourceInventory};
use crate::parser::{strip_comment, without_bom, Header, Keyword, Kind, ParseError, SourceMap};
use crate::paths::{inspect_paths, PathError, PathTemplate};

pub(crate) use self::coverage::check_inventory;
pub(crate) use self::coverage::collect_rule_errors;
use self::coverage::SkippedGroup;
use self::discover::{discover, locate_sources, with_source_paths};
pub use self::discover::{discover_source_files, discover_sources, Discovery};

/// A recipe's rules and any inventory records written with them.
#[derive(Clone, Debug, Default)]
pub struct InputSpec {
    /// The pipeline a recipe's `pipeline analysis.spit` line names: relative
    /// to the recipe's folder when parsed at a path.
    pub pipeline: Option<PathBuf>,
    pub rules: InputRules,
    pub inventory: Option<SourceInventory>,
}

/// Parse a `.spitin` file without resolving imports.
pub fn parse_input_spec(text: &str) -> Result<InputSpec, ParseError> {
    parse_recipe_lines(text).map(|(spec, _)| spec)
}

/// Parse a `.spitin` file at `path`. Paths inside the recipe, and the
/// pipeline it names, are relative to its folder unless the CLI supplies
/// `--root`.
pub fn parse_input_spec_at(text: &str, path: &Path) -> Result<InputSpec, ParseError> {
    let (pipeline, document) = parse_recipe(text, |text| {
        parse_located_document(text, path, Kind::Recipe)
    })?;
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    finish_spec(document, pipeline.map(|pipeline| folder.join(pipeline)))
}

/// Parse a `.spitin` without resolving imports, with where each of its
/// rules is written, for diagnostics.
pub(crate) fn parse_recipe_lines(text: &str) -> Result<(InputSpec, SourceMap), ParseError> {
    let (pipeline, mut document) = parse_recipe(text, |text| {
        parse_document_with_imports(text, &BTreeMap::new(), Kind::Recipe)
    })?;
    let lines = std::mem::take(&mut document.lines);
    Ok((finish_spec(document, pipeline)?, lines))
}

/// A recipe's `pipeline` line, and the document its other lines parse to
/// with `parse` once each is checked to belong in a recipe.
fn parse_recipe(
    text: &str,
    parse: impl FnOnce(&str) -> Result<ParsedDocument, ParseError>,
) -> Result<(Option<PathBuf>, ParsedDocument), ParseError> {
    let (pipeline, text) = pipeline_line(without_bom(text))?;
    check_input_lines(&text)?;
    Ok((pipeline, parse(&text)?))
}

/// The pipeline a recipe names with `pipeline analysis.spit`, and the text
/// with that line blanked so that other lines keep their numbers.
fn pipeline_line(text: &str) -> Result<(Option<PathBuf>, String), ParseError> {
    let mut pipeline = None;
    let mut rest = String::new();
    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        match line.strip_prefix("pipeline ") {
            Some(file) => {
                if pipeline.is_some() {
                    return Err(ParseError::new(
                        index + 1,
                        "a .spitin names its pipeline once",
                    ));
                }
                pipeline = Some(PathBuf::from(file.trim()));
            }
            None => rest.push_str(original),
        }
        rest.push('\n');
    }
    Ok((pipeline, rest))
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
                    | Keyword::Stage
                    | Keyword::Use
            )
        ) || matches!(
            Header::of(line),
            Some(Header::Products | Header::Operations | Header::Pipeline | Header::Commands)
        );
        if pipeline_only {
            return Err(ParseError::new(
                index + 1,
                "logical sources, operations, commands, stages, and imports belong in the .spit pipeline",
            ));
        }
        if line.starts_with("path:") {
            return Err(ParseError::new(
                index + 1,
                "a .spitin path must name a source product, for example `path image: ...`",
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
/// `pipeline`.
fn finish_spec(
    document: ParsedDocument,
    pipeline_file: Option<PathBuf>,
) -> Result<InputSpec, ParseError> {
    let ParsedDocument {
        pipeline,
        mut inputs,
        inventory,
        ..
    } = document;
    if !pipeline.products.is_empty()
        || !pipeline.operations.is_empty()
        || !pipeline.invocations.is_empty()
        || !pipeline.commands.is_empty()
        || !pipeline.stages.is_empty()
        || pipeline.path_template.is_some()
    {
        return Err(ParseError::new(1, "a .spitin file may contain discovery, coverage, source paths, and inventory records only"));
    }
    inputs.source_paths = pipeline.product_paths;
    Ok(InputSpec {
        pipeline: pipeline_file,
        rules: inputs,
        inventory,
    })
}

impl InputSpec {
    /// Check the recipe against the pipeline's source declarations, without
    /// reading any file or record.
    pub fn check(&self, pipeline: &Pipeline) -> Result<(), InputError> {
        for name in self.rules.source_paths.keys() {
            let product = name.clone();
            if !pipeline.is_source(name) {
                return Err(InputError::NotASource { product });
            }
            if pipeline.product_paths.contains_key(name) {
                return Err(InputError::PathInBoth { product });
            }
        }
        if !self.rules.discoveries.is_empty() {
            for product in &pipeline.products {
                if pipeline.is_source(&product.name)
                    && !self.rules.source_paths.contains_key(&product.name)
                    && pipeline.path_template_for(&product.name).is_none()
                {
                    let product = product.name.clone();
                    return Err(InputError::NoDiscoveryPath { product });
                }
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

    /// Run the input stage: find the contexts and source files a dataset
    /// holds, apply the `skip` rules, check the `require` rules, and give
    /// each source record its file's path.
    ///
    /// The stage reads `pipeline` only for its source products and leaves it
    /// untouched. What it returns is a plain inventory, so resolving jobs
    /// never sees a discovery, coverage or skip rule.
    pub fn resolve(
        &self,
        pipeline: &Pipeline,
        source: InputSource<'_>,
    ) -> Result<ResolvedInputs, InputError> {
        validate_pipeline(pipeline)?;
        self.check(pipeline)?;
        let (mut inventory, mut skipped, root) = match source {
            InputSource::Discover(root) => {
                let located = with_source_paths(pipeline, &self.rules.source_paths);
                let found = discover(&located, &self.rules, root)?;
                (found.inventory, found.skipped, Some(root.to_owned()))
            }
            InputSource::Inventory(inventory) => (inventory, Vec::new(), None),
        };
        self.merge_source_paths(pipeline, &mut inventory)?;
        let located = with_source_paths(pipeline, &inventory.source_paths);
        inspect_paths(&located)?;
        let mut checked = check_inventory(pipeline, &self.rules, &inventory)?;
        skipped.extend(checked.skipped.iter().map(SkippedGroup::note));
        locate_sources(&located, &mut checked.inventory)?;
        Ok(ResolvedInputs {
            inventory: checked.inventory,
            skipped,
            gaps: checked.gaps,
            root,
        })
    }

    /// Add the recipe's source path rules to those `inventory` carries, as a
    /// `.spitout` does, so the inventory holds every rule for its sources.
    /// Each must name a source the pipeline declares no rule for, and the
    /// two files must not disagree.
    fn merge_source_paths(
        &self,
        pipeline: &Pipeline,
        inventory: &mut SourceInventory,
    ) -> Result<(), InputError> {
        for (name, template) in &self.rules.source_paths {
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
        for name in inventory.source_paths.keys() {
            let product = name.clone();
            if !pipeline.is_source(name) {
                return Err(InputError::UnknownSourcePath { product });
            }
            if pipeline.product_paths.contains_key(name) {
                return Err(InputError::InventoryPathInBoth { product });
            }
        }
        Ok(())
    }

    /// Give a pipeline the recipe's source paths, and the built-in output
    /// path when it declares none, so an editor can check every path rule of
    /// the two files together. Resolving jobs needs neither: the input
    /// stage writes each source's path into its record.
    pub fn apply_paths(&self, pipeline: &mut Pipeline) {
        pipeline
            .product_paths
            .extend(self.rules.source_paths.clone());
        pipeline
            .path_template
            .get_or_insert_with(PathTemplate::default_output);
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
    /// The recipe sets a path for a product that is not a source.
    NotASource { product: String },
    /// A source has a path rule in both the pipeline and the recipe.
    PathInBoth { product: String },
    /// Directory discovery finds every source's files, and this one has no
    /// path rule to find them by.
    NoDiscoveryPath { product: String },
    /// The recipe and the `.spitout` give a source different path rules.
    ConflictingSourcePaths { product: String },
    /// A `.spitout` gives a path rule for a product that is not a source.
    UnknownSourcePath { product: String },
    /// A source has a path rule in both the pipeline and the `.spitout`.
    InventoryPathInBoth { product: String },
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
            Self::PathInBoth { product } => write!(
                f,
                "source `{product}` has path rules in both .spit and .spitin"
            ),
            Self::NoDiscoveryPath { product } => write!(
                f,
                "source `{product}` needs a path rule in .spitin for directory discovery"
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
    /// The contexts and sources that remain after `skip` rules, with the named
    /// discovery contexts kept for the `.spitout`.
    pub inventory: SourceInventory,
    /// Each file or group left out, and why.
    pub skipped: Vec<String>,
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
