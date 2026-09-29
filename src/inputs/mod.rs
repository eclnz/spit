//! The input stage: settle which contexts and sources a dataset holds.
//!
//! It reads a recipe of `discover`, `require`, `skip` and source path rules,
//! whether written in a `.spitin` file or beside the pipeline in a `.spit`
//! document, and the pipeline's source declarations. It scans a root or takes
//! records already written, and returns a plain inventory with what it
//! skipped and what the `require` rules find missing. Resolving jobs needs
//! nothing else from it.

mod coverage;
mod discover;

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

use crate::compile::validate_pipeline;
use crate::error::ResolveError;
use crate::lower::Document;
use crate::model::{ArtifactInstance, CoverageGap, InputRules, Pipeline, SourceInventory};
use crate::parser::ParseError;
use crate::paths::PathTemplate;
use crate::{parse_spit, parse_spit_at};

pub(crate) use self::coverage::check_inventory;
pub(crate) use self::coverage::collect_rule_errors;
use self::discover::locate_sources;
pub use self::discover::{discover_source_files, discover_sources, Discovery};

/// A recipe's rules and any inventory records written with them.
#[derive(Clone, Debug, Default)]
pub struct InputSpec {
    pub rules: InputRules,
    pub inventory: Option<SourceInventory>,
}

/// Parse a `.spitin` file without resolving imports.
pub fn parse_input_spec(text: &str) -> Result<InputSpec, ParseError> {
    check_input_lines(text)?;
    finish_spec(parse_spit(text)?)
}

/// Parse a `.spitin` file at `path`. Paths inside the recipe are relative to
/// its containing directory unless the CLI supplies `--root`.
pub fn parse_input_spec_at(text: &str, path: &Path) -> Result<InputSpec, ParseError> {
    check_input_lines(text)?;
    finish_spec(parse_spit_at(text, path)?)
}

fn check_input_lines(text: &str) -> Result<(), ParseError> {
    for (index, original) in text.lines().enumerate() {
        let line = original.trim_start();
        if line.starts_with("source ")
            || line.starts_with("operation ")
            || line.starts_with("command ")
            || line.starts_with("verify ")
            || line.starts_with("stage ")
            || line.starts_with("use ")
            || matches!(
                line,
                "products:" | "operations:" | "pipeline:" | "commands:"
            )
        {
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
    }
    Ok(())
}

/// A recipe holds rules and records, and paths only for sources.
fn finish_spec(document: Document) -> Result<InputSpec, ParseError> {
    let Document {
        pipeline,
        mut inputs,
        inventory,
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
        rules: inputs,
        inventory,
    })
}

impl InputSpec {
    /// The rules and records a `.spit` document writes beside its pipeline.
    pub fn embedded_in(document: &Document) -> Self {
        Self {
            rules: document.inputs.clone(),
            inventory: document.inventory.clone(),
        }
    }

    /// Add the rules and records of `other`, such as a `.spitin` recipe's to
    /// those written in the pipeline document. No name may be given twice.
    pub fn merge(&mut self, other: Self) -> Result<(), String> {
        for rule in &other.rules.discoveries {
            if self.rules.discovery(&rule.name).is_some() {
                return Err(format!(
                    "discovery `{}` is declared in both .spit and .spitin",
                    rule.name
                ));
            }
        }
        for name in other.rules.source_paths.keys() {
            if self.rules.source_paths.contains_key(name) {
                return Err(format!("source `{name}` has two .spitin path rules"));
            }
        }
        if self.inventory.is_some() && other.inventory.is_some() {
            return Err("inventory records are written in both .spit and .spitin".into());
        }
        self.rules.discoveries.extend(other.rules.discoveries);
        self.rules.constraints.extend(other.rules.constraints);
        self.rules.source_paths.extend(other.rules.source_paths);
        self.inventory = self.inventory.take().or(other.inventory);
        Ok(())
    }

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
        let (inventory, skipped, root) = match source {
            InputSource::Discover(root) => {
                let found = discover_source_files(pipeline, &self.rules, root)?;
                (found.inventory, found.skipped, Some(root.to_owned()))
            }
            InputSource::Inventory(inventory) => (inventory, Vec::new(), None),
        };
        let mut checked = check_inventory(pipeline, &self.rules, &inventory)?;
        locate_sources(pipeline, &self.rules, &mut checked.inventory)?;
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
    /// Use records already written, from `--sources` or the recipe itself.
    Inventory(SourceInventory),
}

/// What the input stage settled about a dataset.
#[derive(Debug)]
pub struct ResolvedInputs {
    /// The contexts and sources that remain after `skip` rules, with the named
    /// discovery contexts kept for `spit discover`.
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
