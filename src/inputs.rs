//! Dataset input recipes kept separate from the logical pipeline.

use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};

use crate::error::ResolveError;
use crate::model::{Pipeline, SourceInventory};
use crate::parser::ParseError;
use crate::paths::{discover_source_files, PathTemplate};
use crate::resolver::{check_inventory, validate_pipeline};
use crate::{parse_document, parse_document_at};

/// A `.spitin` recipe and any inventory records written in it.
#[derive(Clone, Debug)]
pub struct InputSpec {
    rules: Pipeline,
    pub inventory: Option<SourceInventory>,
}

/// Parse a `.spitin` file without resolving imports.
pub fn parse_input_spec(text: &str) -> Result<InputSpec, ParseError> {
    check_input_lines(text)?;
    let (rules, inventory) = parse_document(text)?;
    finish_spec(rules, inventory)
}

/// Parse a `.spitin` file at `path`. Paths inside the recipe are relative to
/// its containing directory unless the CLI supplies `--root`.
pub fn parse_input_spec_at(text: &str, path: &Path) -> Result<InputSpec, ParseError> {
    check_input_lines(text)?;
    let (rules, inventory) = parse_document_at(text, path)?;
    finish_spec(rules, inventory)
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

fn finish_spec(
    rules: Pipeline,
    inventory: Option<SourceInventory>,
) -> Result<InputSpec, ParseError> {
    if !rules.products.is_empty()
        || !rules.operations.is_empty()
        || !rules.invocations.is_empty()
        || !rules.commands.is_empty()
        || !rules.stages.is_empty()
        || rules.path_template.is_some()
    {
        return Err(ParseError::new(1, "a .spitin file may contain discovery, coverage, source paths, and inventory records only"));
    }
    Ok(InputSpec { rules, inventory })
}

impl InputSpec {
    /// Run the input stage: find the contexts and source files a dataset
    /// holds, apply the recipe's `skip` rules, and check its `require` rules.
    ///
    /// The stage reads the logical `pipeline` only for its source products and
    /// leaves it untouched. What it returns is a plain inventory, so resolving
    /// jobs never sees a discovery, coverage or skip rule.
    pub fn resolve(
        &self,
        pipeline: &Pipeline,
        source: InputSource<'_>,
    ) -> Result<ResolvedInputs, Box<dyn Error>> {
        let mut recipe = pipeline.clone();
        self.apply_to(&mut recipe)?;
        validate_pipeline(&recipe)?;
        let (inventory, skipped, root) = match source {
            InputSource::Discover(root) => {
                let found = discover_source_files(&recipe, root)?;
                (found.inventory, found.skipped, Some(root.to_owned()))
            }
            InputSource::Inventory(inventory) => (inventory, Vec::new(), None),
        };
        let checked = check_inventory(&recipe, &inventory)?;
        Ok(ResolvedInputs {
            inventory: checked.inventory,
            skipped,
            missing: checked.coverage.into_iter().map(|gap| gap.error).collect(),
            root,
        })
    }

    /// Check the recipe against the pipeline's source products.
    fn check_against(&self, pipeline: &Pipeline) -> Result<(), String> {
        let sources: BTreeSet<_> = pipeline
            .products
            .iter()
            .map(|product| product.name.as_str())
            .collect();
        let outputs: BTreeSet<_> = pipeline
            .invocations
            .iter()
            .flat_map(|step| step.outputs.iter().map(String::as_str))
            .collect();
        for name in self.rules.product_paths.keys() {
            if !sources.contains(name.as_str()) || outputs.contains(name.as_str()) {
                return Err(format!(
                    "input path `{name}` must name a source product in the pipeline"
                ));
            }
            if pipeline.product_paths.contains_key(name) {
                return Err(format!(
                    "source `{name}` has path rules in both .spit and .spitin"
                ));
            }
        }
        for rule in &self.rules.discoveries {
            if pipeline
                .discoveries
                .iter()
                .any(|existing| existing.name == rule.name)
            {
                return Err(format!(
                    "discovery `{}` is declared in both .spit and .spitin",
                    rule.name
                ));
            }
        }
        if !self.rules.discoveries.is_empty() && pipeline.path_template.is_none() {
            for name in &sources {
                if !outputs.contains(name)
                    && !self.rules.product_paths.contains_key(*name)
                    && !pipeline.product_paths.contains_key(*name)
                {
                    return Err(format!(
                        "source `{name}` needs a path rule in .spitin for directory discovery"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Give a pipeline the recipe's source paths, and the built-in output
    /// path when it declares none, so paths can be bound to its jobs. This
    /// adds no discovery, coverage or skip rule.
    pub fn apply_paths(&self, pipeline: &mut Pipeline) -> Result<(), String> {
        self.check_against(pipeline)?;
        pipeline
            .product_paths
            .extend(self.rules.product_paths.clone());
        if pipeline.path_template.is_none() {
            pipeline.path_template = Some(
                PathTemplate::parse("out/{product}/{entities}")
                    .expect("built-in output path is valid"),
            );
        }
        Ok(())
    }

    /// Attach every declaration of the recipe to a pipeline, for the input
    /// stage and for diagnostics that check both files together.
    pub fn apply_to(&self, pipeline: &mut Pipeline) -> Result<(), String> {
        self.apply_paths(pipeline)?;
        pipeline.discoveries.extend(self.rules.discoveries.clone());
        pipeline.constraints.extend(self.rules.constraints.clone());
        Ok(())
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
    /// What the recipe's `require` rules find missing.
    pub missing: Vec<ResolveError>,
    /// The directory that was scanned, when the stage scanned one.
    pub root: Option<PathBuf>,
}

impl ResolvedInputs {
    /// The first missing requirement, as an error.
    pub fn require_complete(&self) -> Result<(), ResolveError> {
        self.missing.first().cloned().map_or(Ok(()), Err)
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
