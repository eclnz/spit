//! The input stage: settle which contexts and sources a dataset holds.
//!
//! It reads a `.spitin` recipe of `discover`, `require`, `skip` and source
//! path rules, and the pipeline's source declarations. It scans a root or
//! takes records already written, and returns a plain inventory with what it
//! skipped and what the `require` rules find missing. Resolving jobs needs
//! nothing else from it.

mod coverage;
mod discover;

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

use crate::compile::validate_pipeline;
use crate::error::ResolveError;
use crate::imports::parse_located_document;
use crate::lower::{parse_document_with_imports, ParsedDocument};
use crate::model::{ArtifactInstance, CoverageGap, InputRules, Pipeline, SourceInventory};
use crate::parser::{strip_comment, Header, Keyword, Kind, ParseError, SourceMap};
use crate::paths::{inspect_paths, PathTemplate};

pub(crate) use self::coverage::check_inventory;
pub(crate) use self::coverage::collect_rule_errors;
use self::coverage::SkippedGroup;
use self::discover::locate_sources;
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
    let (pipeline, text) = pipeline_line(text)?;
    check_input_lines(&text)?;
    let spec = finish_spec(parse_document_with_imports(
        &text,
        &Default::default(),
        Kind::Recipe,
    )?)?;
    Ok(InputSpec { pipeline, ..spec })
}

/// Parse a `.spitin` file at `path`. Paths inside the recipe, and the
/// pipeline it names, are relative to its folder unless the CLI supplies
/// `--root`.
pub fn parse_input_spec_at(text: &str, path: &Path) -> Result<InputSpec, ParseError> {
    let (pipeline, text) = pipeline_line(text)?;
    check_input_lines(&text)?;
    let spec = finish_spec(parse_located_document(&text, path, Kind::Recipe)?)?;
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    Ok(InputSpec {
        pipeline: pipeline.map(|pipeline| folder.join(pipeline)),
        ..spec
    })
}

/// Parse a `.spitin` without resolving imports, with where each of its
/// rules is written, for diagnostics.
pub(crate) fn parse_recipe_lines(text: &str) -> Result<(InputSpec, SourceMap), ParseError> {
    let (pipeline, text) = pipeline_line(text)?;
    check_input_lines(&text)?;
    let mut document = parse_document_with_imports(&text, &Default::default(), Kind::Recipe)?;
    let lines = std::mem::take(&mut document.lines);
    let spec = finish_spec(document)?;
    Ok((InputSpec { pipeline, ..spec }, lines))
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

/// A recipe holds rules and records, and paths only for sources.
fn finish_spec(document: ParsedDocument) -> Result<InputSpec, ParseError> {
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
        pipeline: None,
        rules: inputs,
        inventory,
    })
}

impl InputSpec {
    /// Check the recipe against the pipeline's source declarations, without
    /// reading any file or record.
    pub fn check(&self, pipeline: &Pipeline) -> Result<(), Box<dyn Error>> {
        for name in self.rules.source_paths.keys() {
            if !pipeline.is_source(name) {
                return Err(format!(
                    "input path `{name}` must name a source product in the pipeline"
                )
                .into());
            }
            if pipeline.product_paths.contains_key(name) {
                return Err(
                    format!("source `{name}` has path rules in both .spit and .spitin").into(),
                );
            }
        }
        if !self.rules.discoveries.is_empty() {
            for product in &pipeline.products {
                if pipeline.is_source(&product.name)
                    && !self.rules.source_paths.contains_key(&product.name)
                    && pipeline.path_template_for(&product.name).is_none()
                {
                    return Err(format!(
                        "source `{}` needs a path rule in .spitin for directory discovery",
                        product.name
                    )
                    .into());
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
    ) -> Result<ResolvedInputs, Box<dyn Error>> {
        validate_pipeline(pipeline)?;
        self.check(pipeline)?;
        let (mut inventory, mut skipped, root) = match source {
            InputSource::Discover(root) => {
                let found = discover_source_files(pipeline, &self.rules, root)?;
                (found.inventory, found.skipped, Some(root.to_owned()))
            }
            InputSource::Inventory(inventory) => (inventory, Vec::new(), None),
        };
        for (name, template) in &self.rules.source_paths {
            if let Some(existing) = inventory.source_paths.get(name) {
                if existing != template {
                    return Err(format!(
                        "source `{name}` has conflicting path rules in .spitin and .spitout"
                    )
                    .into());
                }
            }
            inventory
                .source_paths
                .insert(name.clone(), template.clone());
        }
        for name in inventory.source_paths.keys() {
            if !pipeline.is_source(name) {
                return Err(format!("source path rule names unknown source `{name}`").into());
            }
            if pipeline.product_paths.contains_key(name) {
                return Err(
                    format!("source `{name}` has path rules in both .spit and .spitout").into(),
                );
            }
        }
        let mut path_pipeline = pipeline.clone();
        path_pipeline
            .product_paths
            .extend(inventory.source_paths.clone());
        inspect_paths(&path_pipeline)?;
        let mut checked = check_inventory(pipeline, &self.rules, &inventory)?;
        skipped.extend(checked.skipped.iter().map(SkippedGroup::note));
        let mut path_rules = self.rules.clone();
        path_rules.source_paths = checked.inventory.source_paths.clone();
        locate_sources(pipeline, &path_rules, &mut checked.inventory)?;
        Ok(ResolvedInputs {
            inventory: checked.inventory,
            skipped,
            gaps: checked.gaps,
            root,
        })
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
            discovered: Default::default(),
            ..self.inventory.clone()
        }
    }
}
