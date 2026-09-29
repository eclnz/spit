//! Source inventories: `sources:` records and `contexts:`, whether in a
//! `.spitout` or written in a `.spitin` recipe.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::model::{EntityBinding, InputRules, Pipeline, SourceInventory, SourceRecord};
use crate::paths::unusable_path;

use super::keyword::{Header, Keyword};
use super::lexical::{comma_items, identifier, qualified_identifier, strip_comment};
use super::ParseError;

/// A document's text split in two: the rest, and any records
/// under `sources:` or `contexts:` headers. Each keeps the document's line
/// numbers, with blank lines where the other's lines were.
pub(crate) struct DocumentText {
    pub(crate) pipeline: String,
    pub(crate) inventory: String,
    /// The line of the first `sources:` or `contexts:` header.
    pub(crate) inventory_line: Option<usize>,
}

pub(crate) fn split_document(text: &str) -> DocumentText {
    let mut pipeline_text = String::new();
    let mut inventory_text = String::new();
    let mut inventory_section = false;
    let mut inventory_line = None;

    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        // Records run from a `sources:` or `contexts:` header to the next
        // header, statement or step.
        match Header::of(line) {
            Some(header) if header.is_records() => {
                inventory_section = true;
                inventory_line.get_or_insert(index + 1);
            }
            Some(_) => inventory_section = false,
            None if Keyword::of(line).is_some() || is_step(line) => inventory_section = false,
            None => {}
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

/// Whether `line` reads as a step, `output = operation(inputs)`, rather
/// than a record such as `image[subject=A]` or a context `[subject=A]`.
fn is_step(line: &str) -> bool {
    let record = line.starts_with('[')
        || line.find('[').is_some_and(|bracket| {
            qualified_identifier(line[..bracket].trim(), 0, "product").is_ok()
        });
    !record
        && line
            .find('(')
            .is_some_and(|paren| line[..paren].contains('='))
}

/// Write an inventory in the text form [`parse_source_inventory`] reads,
/// with each record's values in its product's declared dimension order.
///
/// A discovery rule's contexts follow the dimensions the rule declares, and
/// other contexts the order the pipeline first declares its dimensions in.
/// Contexts are listed in the order their values are written, reading
/// numbers as numbers.
pub fn render_source_inventory(
    inventory: &SourceInventory,
    pipeline: &Pipeline,
    rules: &InputRules,
) -> String {
    let mut pipeline_order: Vec<String> = Vec::new();
    for dimension in pipeline
        .products
        .iter()
        .flat_map(|product| &product.dimensions)
    {
        if !pipeline_order.contains(dimension) {
            pipeline_order.push(dimension.clone());
        }
    }
    let context_lines = |contexts: Vec<&EntityBinding>, first: &[String]| {
        let mut order = first.to_vec();
        order.extend(
            pipeline_order
                .iter()
                .filter(|d| !first.contains(d))
                .cloned(),
        );
        let mut contexts = contexts;
        contexts.sort_by(|left, right| left.cmp_in(right, &order));
        let order: Vec<&str> = order.iter().map(String::as_str).collect();
        contexts
            .into_iter()
            .map(|context| format!("    [{}]\n", in_order(context, &order)))
            .collect::<String>()
    };
    let mut text = String::new();
    let named: BTreeSet<_> = inventory.discovered.values().flatten().collect();
    let unnamed: Vec<_> = inventory
        .contexts
        .iter()
        .filter(|context| !named.contains(context))
        .collect();
    if !unnamed.is_empty() {
        text.push_str("contexts:\n");
        text.push_str(&context_lines(unnamed, &[]));
    }
    for (name, bindings) in &inventory.discovered {
        text.push_str(&format!("contexts {name}:\n"));
        let declared = rules
            .discovery(name)
            .map_or(&[][..], |rule| rule.dimensions.as_slice());
        text.push_str(&context_lines(bindings.iter().collect(), declared));
    }
    text.push_str("sources:\n");
    for record in &inventory.artifacts {
        let declared: Vec<_> = pipeline
            .products
            .iter()
            .find(|product| product.name == record.product)
            .map_or(Vec::new(), |product| {
                product.dimensions.iter().map(String::as_str).collect()
            });
        text.push_str(&format!(
            "    {}[{}]",
            record.product,
            in_order(&record.entities, &declared)
        ));
        if let Some(path) = &record.path {
            text.push_str(&format!(": {path}"));
        }
        text.push('\n');
    }
    text
}

/// `binding` as `dim=value,...`, in the order of `declared`, then any
/// dimension it does not name.
fn in_order(binding: &EntityBinding, declared: &[&str]) -> String {
    let mut values: Vec<_> = binding.0.iter().collect();
    values.sort_by_key(|(dimension, _)| {
        declared
            .iter()
            .position(|declared| declared == dimension)
            .unwrap_or(usize::MAX)
    });
    values
        .into_iter()
        .map(|(dimension, value)| format!("{dimension}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The pipeline an inventory names with a `pipeline analysis.spit` line
/// before its first section, as written by `spit inputs`, exactly as
/// written. An inventory from a dataset indexer or a fixture names none.
pub fn inventory_pipeline(text: &str) -> Result<Option<PathBuf>, ParseError> {
    let mut pipeline = None;
    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        if Header::of(line).is_some() {
            break;
        }
        if let Some(file) = line.strip_prefix("pipeline ") {
            if pipeline.is_some() {
                return Err(
                    ParseError::new(index + 1, "an inventory names its pipeline once")
                        .locate(original),
                );
            }
            pipeline = Some(PathBuf::from(file.trim()));
        }
    }
    Ok(pipeline)
}

/// Parse an inventory supplied by a dataset indexer or written as a fixture.
/// Records are logical identities, each optionally followed by `: path`, the
/// file relative to the dataset root. They never hold artifact types. A
/// `pipeline` line before the first section is read by [`inventory_pipeline`]
/// and skipped here.
pub fn parse_source_inventory(text: &str) -> Result<SourceInventory, ParseError> {
    enum InventorySection {
        Sources,
        Contexts(Option<String>),
    }

    let mut inventory = SourceInventory::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }
        if section.is_none() && line.starts_with("pipeline ") {
            continue;
        }
        match Header::of(line) {
            Some(Header::Sources) => section = Some(InventorySection::Sources),
            Some(Header::Contexts(None)) => section = Some(InventorySection::Contexts(None)),
            Some(Header::Contexts(Some(name))) => {
                let name = identifier(name, number, "discovery name")
                    .map_err(|error| error.locate(original))?;
                section = Some(InventorySection::Contexts(Some(name.to_owned())));
            }
            _ => match section {
                Some(InventorySection::Sources) => {
                    let record = parse_source(line, number).map_err(|e| e.locate(original))?;
                    inventory.artifacts.push(record);
                }
                Some(InventorySection::Contexts(ref name)) => {
                    let context = parse_context(line, number).map_err(|e| e.locate(original))?;
                    inventory.contexts.push(context.clone());
                    if let Some(name) = name {
                        inventory
                            .discovered
                            .entry(name.clone())
                            .or_default()
                            .push(context);
                    }
                }
                None => {
                    return Err(ParseError::new(
                        number,
                        "expected inventory section header: sources:, contexts:, or contexts name:",
                    )
                    .locate(original))
                }
            },
        }
    }
    inventory.contexts.sort();
    inventory.contexts.dedup();
    for bindings in inventory.discovered.values_mut() {
        bindings.sort();
        bindings.dedup();
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
    // A value holds no `]`, so the first one closes the record.
    let (bindings, path) = match bindings.find(']') {
        Some(end) => bindings.split_at(end + 1),
        None => (bindings, ""),
    };
    let entities = parse_bindings(bindings, number)?;
    let record = SourceRecord::new(product, entities);
    let path = path.trim();
    if path.is_empty() {
        return Ok(record);
    }
    let path = path
        .strip_prefix(':')
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            ParseError::new(number, "expected `: path` after a source record").at_token(path)
        })?;
    if let Some(reason) = unusable_path(path) {
        return Err(ParseError::new(number, format!("source path {reason}")).at_token(path));
    }
    Ok(record.at(path))
}

/// Locate valid source records in an inventory without parsing each line as
/// a separate document. Used only to place diagnostics after a failed check.
pub(crate) fn source_record_lines(text: &str) -> Vec<(usize, SourceRecord)> {
    let mut sources = false;
    let mut records = Vec::new();
    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        match Header::of(line) {
            Some(Header::Sources) => sources = true,
            Some(Header::Contexts(_)) => sources = false,
            _ if sources => {
                if let Ok(record) = parse_source(line, index + 1) {
                    records.push((index + 1, record));
                }
            }
            _ => {}
        }
    }
    records
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
