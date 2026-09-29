//! Fixtures written as one text: a pipeline, then its records under
//! `sources:` or `contexts:`. SPIT reads the two from separate files, so
//! these split them first.

#![allow(dead_code)]

use spit::{parse_pipeline, parse_source_inventory, ParseError, Pipeline, SourceInventory};

/// The pipeline part of `text`, and its records, if any. The pipeline keeps
/// its line numbers; the records are numbered from their own first line.
pub fn split(text: &str) -> (String, Option<String>) {
    let lines: Vec<_> = text.lines().collect();
    let start = lines.iter().position(|line| {
        let line = line.trim();
        line == "sources:"
            || line == "contexts:"
            || (line.starts_with("contexts ") && line.ends_with(':'))
    });
    match start {
        None => (text.to_owned(), None),
        Some(start) => (
            lines[..start].join("\n") + "\n",
            Some(lines[start..].join("\n") + "\n"),
        ),
    }
}

/// The pipeline and records of a fixture, each parsed as its own file is.
pub fn parse_fixture(text: &str) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    let (pipeline, records) = split(text);
    let records = records
        .map(|records| parse_source_inventory(&records))
        .transpose()?;
    Ok((parse_pipeline(&pipeline)?, records))
}

/// Whether `line` is a `discover`, `require` or `skip` rule.
fn is_rule(line: &str) -> bool {
    let line = line.trim_start();
    line.trim_end() == "constraints:"
        || ["discover ", "require ", "skip "]
            .iter()
            .any(|keyword| line.starts_with(keyword))
}

/// The pipeline part of `text` with its rule lines blanked, so it keeps its
/// line numbers, and those rules as the text of a recipe.
pub fn split_rules(text: &str) -> (String, String) {
    let mut pipeline = String::new();
    let mut recipe = String::new();
    for line in text.lines() {
        if is_rule(line) {
            if line.trim() != "constraints:" {
                recipe.push_str(line.trim_start());
                recipe.push('\n');
            }
        } else {
            pipeline.push_str(line);
        }
        pipeline.push('\n');
    }
    (pipeline, recipe)
}

/// A fixture's pipeline, its rules as a recipe, and its records.
pub fn parse_with_rules(
    text: &str,
) -> Result<(Pipeline, spit::InputSpec, Option<SourceInventory>), ParseError> {
    let (pipeline, records) = split(text);
    let (pipeline, recipe) = split_rules(&pipeline);
    let (pipeline, records) = parse_fixture(&(pipeline + &records.unwrap_or_default()))?;
    Ok((pipeline, spit::parse_input_spec(&recipe)?, records))
}

/// As [`parse_fixture`], for a pipeline at `path`, whose imports resolve
/// from its folder.
pub fn parse_fixture_at(
    text: &str,
    path: &std::path::Path,
) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    let (pipeline, records) = split(text);
    let records = records
        .map(|records| parse_source_inventory(&records))
        .transpose()?;
    Ok((spit::parse_pipeline_at(&pipeline, path)?, records))
}
