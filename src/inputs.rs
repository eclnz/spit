//! Dataset input recipes kept separate from the logical pipeline.

use std::collections::BTreeSet;
use std::path::Path;

use crate::model::{Pipeline, SourceInventory};
use crate::parser::ParseError;
use crate::paths::PathTemplate;
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
    /// Attach the recipe's declarations to a pipeline without replacing its
    /// logical source contracts. Existing path rules must be unambiguous.
    pub fn apply_to(&self, pipeline: &mut Pipeline) -> Result<(), String> {
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
        pipeline.discoveries.extend(self.rules.discoveries.clone());
        pipeline.constraints.extend(self.rules.constraints.clone());
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
}
