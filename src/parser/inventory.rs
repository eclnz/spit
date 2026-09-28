//! Source inventories: `sources:` records and `contexts:`, whether in their
//! own text or inline in a pipeline document.

use std::collections::BTreeMap;

use crate::model::{EntityBinding, Pipeline, SourceInventory, SourceRecord};

use super::flow::is_stage_header;
use super::lexical::{comma_items, identifier, qualified_identifier, strip_comment};
use super::ParseError;

/// A document's text split in two: the pipeline, and any inline inventory
/// under `sources:` or `contexts:` headers. Each keeps the document's line
/// numbers, with blank lines where the other's lines were.
pub(crate) struct DocumentText {
    pub(crate) pipeline: String,
    pub(crate) inventory: String,
    /// The line of the first inline `sources:` or `contexts:` header.
    pub(crate) inventory_line: Option<usize>,
}

pub(crate) fn split_document(text: &str) -> DocumentText {
    let mut pipeline_text = String::new();
    let mut inventory_text = String::new();
    let mut inventory_section = false;
    let mut inventory_line = None;

    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        match line {
            "products:" | "operations:" | "pipeline:" | "constraints:" | "commands:" => {
                inventory_section = false;
            }
            "sources:" | "contexts:" => {
                inventory_section = true;
                inventory_line.get_or_insert(index + 1);
            }
            _ if line.starts_with("path:")
                || line.starts_with("path ")
                || line.starts_with("shell-source:")
                || line.starts_with("use ")
                || line.starts_with("source ")
                || line.starts_with("operation ")
                || line.starts_with("command ")
                || line.starts_with("verify ")
                || line.starts_with("require ")
                || is_stage_header(line) =>
            {
                inventory_section = false;
            }
            _ => {}
        }
        // Each line goes to one side and a blank line to the other, so both
        // texts keep the document's line numbers.
        let target = if inventory_section {
            &mut inventory_text
        } else {
            &mut pipeline_text
        };
        target.push_str(original);
        pipeline_text.push('\n');
        inventory_text.push('\n');
    }
    DocumentText {
        pipeline: pipeline_text,
        inventory: inventory_text,
        inventory_line,
    }
}

/// Write an inventory in the text form [`parse_source_inventory`] reads,
/// with each record's values in its product's declared dimension order.
pub fn render_source_inventory(inventory: &SourceInventory, pipeline: &Pipeline) -> String {
    let mut text = String::new();
    if !inventory.contexts.is_empty() {
        text.push_str("contexts:\n");
        for context in &inventory.contexts {
            text.push_str(&format!("    [{context}]\n"));
        }
    }
    text.push_str("sources:\n");
    for record in &inventory.artifacts {
        let declared = pipeline
            .products
            .iter()
            .find(|product| product.name == record.product)
            .map_or(&[][..], |product| product.dimensions.as_slice());
        let mut values: Vec<_> = record.entities.0.iter().collect();
        values.sort_by_key(|(dimension, _)| {
            declared
                .iter()
                .position(|declared| declared == *dimension)
                .unwrap_or(usize::MAX)
        });
        let values: Vec<_> = values
            .into_iter()
            .map(|(dimension, value)| format!("{dimension}={value}"))
            .collect();
        text.push_str(&format!("    {}[{}]\n", record.product, values.join(",")));
    }
    text
}

/// Parse an inventory supplied by a dataset indexer or written as a fixture.
/// The inventory contains logical identities, never paths or artifact types.
pub fn parse_source_inventory(text: &str) -> Result<SourceInventory, ParseError> {
    enum InventorySection {
        Sources,
        Contexts,
    }

    let mut inventory = SourceInventory::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "sources:" => section = Some(InventorySection::Sources),
            "contexts:" => section = Some(InventorySection::Contexts),
            _ => match section {
                Some(InventorySection::Sources) => {
                    let record = parse_source(line, number).map_err(|e| e.locate(original))?;
                    inventory.artifacts.push(record);
                }
                Some(InventorySection::Contexts) => {
                    let context = parse_context(line, number).map_err(|e| e.locate(original))?;
                    inventory.contexts.push(context);
                }
                None => {
                    return Err(ParseError::new(
                        number,
                        "expected inventory section header: sources: or contexts:",
                    )
                    .locate(original))
                }
            },
        }
    }
    Ok(inventory)
}

fn parse_source(line: &str, number: usize) -> Result<SourceRecord, ParseError> {
    let (product, bindings) = line.split_once('[').ok_or_else(|| {
        ParseError::new(
            number,
            "expected source artifact: product[dimension=value,...]",
        )
    })?;
    let product = qualified_identifier(product.trim(), number, "source product")?;
    let entities = parse_bindings(bindings, number)?;
    Ok(SourceRecord::new(product, entities))
}

fn parse_context(line: &str, number: usize) -> Result<EntityBinding, ParseError> {
    let bindings = line
        .strip_prefix('[')
        .ok_or_else(|| ParseError::new(number, "expected context: [dimension=value,...]"))?;
    parse_bindings(bindings, number)
}

/// Parse `dimension=value, ...]`, the text after a record's opening `[`.
fn parse_bindings(bindings: &str, number: usize) -> Result<EntityBinding, ParseError> {
    let open = bindings.trim_end();
    let bindings = open.strip_suffix(']').ok_or_else(|| {
        let error = ParseError::new(number, "expected closing `]` in source artifact");
        if open.is_empty() {
            error
        } else {
            error.at_token(open)
        }
    })?;
    let mut values = BTreeMap::new();
    for item in comma_items(bindings, number)? {
        let (dimension, value) = item.split_once('=').ok_or_else(|| {
            ParseError::new(number, "expected `dimension=value` in source artifact")
        })?;
        let dimension = identifier(dimension.trim(), number, "source dimension")?;
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(ParseError::new(
                number,
                "source dimension value must be one nonempty token",
            )
            .at_token(item));
        }
        if values
            .insert(dimension.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(ParseError::new(
                number,
                format!("duplicate source dimension `{dimension}`"),
            )
            .at_token(item));
        }
    }
    Ok(EntityBinding(values))
}
