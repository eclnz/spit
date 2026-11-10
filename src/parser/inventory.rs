//! Source inventories: `sources:` records and `contexts:`, whether in a
//! `.spitout` or written in a `.spitin` recipe.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::model::{EntityBinding, Removal, SourceInventory, SourceRecord, Verdict, VerdictRow};
use crate::template::PathTemplate;

use super::keyword::Header;
use super::layout::{Head, Layout, Line};
use super::lexical::{comma_items, identifier, qualified_identifier};
use super::ParseError;

/// Parse an inventory supplied by a dataset indexer or written as a fixture.
/// Records are logical identities, each optionally followed by `: path`, the
/// file relative to the dataset root. They never hold artifact types.
pub fn parse_source_inventory(text: &str) -> Result<SourceInventory, ParseError> {
    match parse_inventory_recovering(text) {
        (inventory, errors) if errors.is_empty() => Ok(inventory),
        (_, errors) => Err(errors
            .into_iter()
            .next()
            .expect("an inventory that fails has an error")),
    }
}

/// As [`parse_source_inventory`], in one parse: the records of the lines
/// that read, and an error for each line that does not.
pub(crate) fn parse_inventory_recovering(text: &str) -> (SourceInventory, Vec<ParseError>) {
    let layout = Layout::new(super::without_bom(text));
    let parsed = read_inventory(layout.numbered());
    (parsed.inventory, parsed.errors)
}

/// The records of a document laid out in `layout`, read as
/// [`parse_inventory_recovering`] reads an inventory's lines.
pub(crate) fn document_records(layout: &Layout) -> (SourceInventory, Vec<ParseError>) {
    let parsed = read_inventory(layout.numbered().filter(|(_, line)| line.record));
    (parsed.inventory, parsed.errors)
}

/// An inventory as read: its records, each with its line, and an error for
/// each line that does not read.
struct ParsedInventory {
    inventory: SourceInventory,
    record_lines: Vec<(usize, SourceRecord)>,
    errors: Vec<ParseError>,
}

fn read_inventory<'a, 't: 'a>(
    lines: impl Iterator<Item = (usize, &'a Line<'t>)>,
) -> ParsedInventory {
    enum InventorySection {
        Sources,
        SourcePaths,
        Contexts,
        Removed,
        Verdicts,
    }

    let mut inventory = SourceInventory::default();
    let mut record_lines = Vec::new();
    let mut section = None;
    let mut parent: Option<(EntityBinding, usize)> = None;
    let mut group: Option<(Vec<EntityBinding>, usize)> = None;
    let mut errors = Vec::new();
    let mut verdicts: Vec<PendingVerdict> = Vec::new();
    for (number, read) in lines {
        let Line {
            text: original,
            indent,
            code: line,
            head,
            ..
        } = *read;
        if head == Head::Blank {
            continue;
        }
        // A line that fails changes nothing, and the lines after it are
        // read as they would be without it.
        let read = (|| -> Result<(), ParseError> {
            let root = [
                ("root ", "root", "root"),
                ("output ", "output", "output root"),
            ]
            .into_iter()
            .find_map(|(prefix, word, what)| Some((line.strip_prefix(prefix)?, word, what)));
            if let Some((folder, word, what)) = root {
                if section.is_some() {
                    return Err(ParseError::new(
                        number,
                        format!("`{word}` comes before every section of a .spitout"),
                    )
                    .locate(original));
                }
                let slot = if word == "root" {
                    &mut inventory.root
                } else {
                    &mut inventory.output
                };
                if slot.is_some() {
                    return Err(ParseError::new(
                        number,
                        format!("a .spitout names its {what} once"),
                    )
                    .locate(original));
                }
                *slot = Some(PathBuf::from(folder.trim()));
                return Ok(());
            }
            match read.header() {
                Some(Header::Sources) => {
                    section = Some(InventorySection::Sources);
                    parent = None;
                    group = None;
                }
                Some(Header::SourcePaths) => {
                    section = Some(InventorySection::SourcePaths);
                    parent = None;
                    group = None;
                }
                Some(Header::Contexts(None)) => {
                    section = Some(InventorySection::Contexts);
                    parent = None;
                    group = None;
                }
                Some(Header::Removed) => {
                    section = Some(InventorySection::Removed);
                    parent = None;
                    group = None;
                }
                Some(Header::Verdicts) => {
                    section = Some(InventorySection::Verdicts);
                    parent = None;
                    group = None;
                }
                Some(Header::Contexts(Some(name))) => {
                    return Err(ParseError::new(
                        number,
                        format!("named context section `{name}` is replaced by `contexts:`"),
                    )
                    .locate(original));
                }
                _ => match section {
                    Some(InventorySection::Sources) => {
                        if line.contains(['[', ']', '=']) {
                            let record =
                                parse_source(line, number).map_err(|e| e.locate(original))?;
                            record_lines.push((number, record.clone()));
                            inventory.artifacts.push(record);
                        } else {
                            let records =
                                parse_product_list(line, number, &EntityBinding::default())
                                    .map_err(|e| e.locate(original))?;
                            record_lines
                                .extend(records.iter().cloned().map(|record| (number, record)));
                            inventory.artifacts.extend(records);
                        }
                    }
                    Some(InventorySection::Removed) => {
                        parse_removed_line(read, number, &mut inventory.removed)
                            .map_err(|e| e.locate(original))?;
                    }
                    Some(InventorySection::Verdicts) => {
                        parse_verdict_line(read, number, &mut verdicts)
                            .map_err(|e| e.locate(original))?;
                    }
                    Some(InventorySection::SourcePaths) => {
                        let (name, template) = line.split_once(':').ok_or_else(|| {
                            ParseError::new(number, "expected source path: product: template")
                                .locate(original)
                        })?;
                        let name = qualified_identifier(name.trim(), number, "source product")
                            .map_err(|e| e.locate(original))?;
                        let template = PathTemplate::parse(template.trim())
                            .map_err(|e| ParseError::new(number, e.to_string()).locate(original))?;
                        if inventory.source_paths.contains_key(name) {
                            return Err(ParseError::new(
                                number,
                                format!("duplicate source path rule for `{name}`"),
                            )
                            .locate(original));
                        }
                        inventory.source_paths.insert(name.to_owned(), template);
                    }
                    Some(InventorySection::Contexts) => {
                        let nests_under = parent
                            .as_ref()
                            .filter(|(_, at)| line.starts_with('[') && indent > *at);
                        if let Some((context, _)) = nests_under {
                            let values = parse_group(line, number, context)
                                .map_err(|e| e.locate(original))?;
                            group = Some((values, indent));
                        } else if line.starts_with('[') {
                            let context = parse_context(line.trim_end_matches(':'), number)
                                .map_err(|e| e.locate(original))?;
                            inventory.contexts.push(context.clone());
                            parent = line.ends_with(':').then_some((context, indent));
                            group = None;
                        } else {
                            let (context, at) = parent.as_ref().ok_or_else(|| {
                                ParseError::new(number, "source list needs a parent context")
                                    .locate(original)
                            })?;
                            if indent <= *at {
                                return Err(ParseError::new(
                                    number,
                                    "source list must be indented under its context",
                                )
                                .locate(original));
                            }
                            let bases = group
                                .as_ref()
                                .filter(|(_, group_indent)| indent > *group_indent)
                                .map_or_else(
                                    || vec![context.clone()],
                                    |(values, _)| values.clone(),
                                );
                            for base in bases {
                                let records = parse_product_list(line, number, &base)
                                    .map_err(|e| e.locate(original))?;
                                record_lines
                                    .extend(records.iter().cloned().map(|record| (number, record)));
                                inventory.artifacts.extend(records);
                            }
                        }
                    }
                    None => return Err(ParseError::new(
                        number,
                        "expected inventory section header: sources:, contexts:, or contexts name:",
                    )
                    .locate(original)),
                },
            }
            Ok(())
        })();
        if let Err(error) = read {
            errors.push(error);
        }
    }
    for pending in verdicts {
        match pending.finish() {
            Ok(row) => inventory.verdicts.push(row),
            Err(error) => errors.push(error),
        }
    }
    // Keep in step with `as_read_back`, which must normalize a settled
    // inventory as reading its text does here.
    inventory.contexts.sort();
    inventory.contexts.dedup();
    ParsedInventory {
        inventory,
        record_lines,
        errors,
    }
}

/// Read one line of a `removed:` section into `removed`: what was removed,
/// `product[dimension=value,...]` or `[dimension=value,...]`, or one of its
/// `rule:`, `at:` and `reason:` lines. Values are read from the whole line,
/// so a `#` in a reason is kept.
fn parse_removed_line(
    read: &Line,
    number: usize,
    removed: &mut Vec<Removal>,
) -> Result<(), ParseError> {
    let line = read.text.trim();
    if let Some((key, value)) = line
        .split_once(':')
        .filter(|(key, _)| matches!(*key, "rule" | "at" | "reason" | "found"))
    {
        let removal = removed.last_mut().ok_or_else(|| {
            ParseError::new(
                number,
                format!("`{key}:` needs the removed artifact or group above it"),
            )
        })?;
        let value = value.trim().to_owned();
        match key {
            "rule" => removal.rule = value,
            "at" => removal.origin = Some(value),
            "found" => {
                let found = value.parse().map_err(|_| {
                    ParseError::new(number, "`found:` must be a nonnegative integer")
                })?;
                removal.found = Some(found);
            }
            _ => removal.reason = Some(value),
        }
        return Ok(());
    }
    let line = read.code;
    let (product, entities) = if line.starts_with('[') {
        (None, parse_context(line, number)?)
    } else {
        let record = parse_source(line, number)?;
        (Some(record.product), record.entities)
    };
    removed.push(Removal {
        product,
        entities,
        rule: String::new(),
        origin: None,
        reason: None,
        found: None,
    });
    Ok(())
}

/// A `verdicts:` entry as it is read: the row, and whether the lines under
/// it have given its verdict and where it was read from.
struct PendingVerdict {
    row: VerdictRow,
    verdict: Option<Verdict>,
    at: Option<(String, usize)>,
    line: usize,
}

impl PendingVerdict {
    /// The row, once its `verdict:` and `at:` lines are read.
    fn finish(self) -> Result<VerdictRow, ParseError> {
        let (Some(verdict), Some((file, at))) = (self.verdict, self.at) else {
            return Err(ParseError::new(
                self.line,
                "a verdict needs a `verdict: pass` or `verdict: fail` line and an `at: file line N` line",
            ));
        };
        Ok(VerdictRow {
            verdict,
            file,
            line: at,
            ..self.row
        })
    }
}

/// Read one line of a `verdicts:` section into `verdicts`: the review and
/// what it judges, `review[dimension=value,...]`, or one of its `verdict:`,
/// `at:` and `reason:` lines. Values are read from the whole line, so a `#`
/// in a reason is kept.
fn parse_verdict_line(
    read: &Line,
    number: usize,
    verdicts: &mut Vec<PendingVerdict>,
) -> Result<(), ParseError> {
    let line = read.text.trim();
    if let Some((key, value)) = line
        .split_once(':')
        .filter(|(key, _)| matches!(*key, "verdict" | "at" | "reason"))
    {
        let pending = verdicts.last_mut().ok_or_else(|| {
            ParseError::new(
                number,
                format!("`{key}:` needs the review and what it judges above it"),
            )
        })?;
        let value = value.trim();
        match key {
            "verdict" => {
                pending.verdict = Some(match value {
                    "pass" => Verdict::Pass,
                    "fail" => Verdict::Fail,
                    _ => {
                        return Err(ParseError::new(
                            number,
                            "`verdict:` must be `pass` or `fail`",
                        ))
                    }
                });
            }
            "at" => {
                let (file, at) = value
                    .rsplit_once(" line ")
                    .and_then(|(file, at)| Some((file, at.parse().ok()?)))
                    .ok_or_else(|| {
                        ParseError::new(
                            number,
                            "`at:` must be a file and a line, as `qc/looked_at.csv line 2`",
                        )
                    })?;
                pending.at = Some((file.to_owned(), at));
            }
            _ => pending.row.reason = Some(value.to_owned()),
        }
        return Ok(());
    }
    let record = parse_source(read.code, number)?;
    verdicts.push(PendingVerdict {
        row: VerdictRow {
            review: record.product,
            entities: record.entities,
            verdict: Verdict::Pass,
            reason: None,
            file: String::new(),
            line: 0,
        },
        verdict: None,
        at: None,
        line: number,
    });
    Ok(())
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
    let path = path.trim();
    if !path.is_empty() {
        return Err(ParseError::new(
            number,
            format!(
                "a source record names no file; its source's path rule gives it, so remove `{path}`"
            ),
        )
        .at_token(path));
    }
    Ok(SourceRecord::new(product, entities))
}

/// Expand a list of source products under an already bound context.
fn parse_product_list(
    line: &str,
    number: usize,
    entities: &EntityBinding,
) -> Result<Vec<SourceRecord>, ParseError> {
    let names = comma_items(line, number)?;
    if names.is_empty() {
        return Err(ParseError::new(
            number,
            "expected at least one source product",
        ));
    }
    names
        .into_iter()
        .map(|name| {
            qualified_identifier(name.trim(), number, "source product")
                .map(|name| SourceRecord::new(name, entities.clone()))
        })
        .collect()
}

/// Expand `[run=01,02]:` under a parent such as `[sub=01,ses=01]:`.
fn parse_group(
    line: &str,
    number: usize,
    parent: &EntityBinding,
) -> Result<Vec<EntityBinding>, ParseError> {
    let inner = line
        .strip_prefix('[')
        .and_then(|line| line.strip_suffix("]:"))
        .ok_or_else(|| ParseError::new(number, "expected nested group: [dimension=value,...]:"))?;
    let (dimension, values) = inner
        .split_once('=')
        .ok_or_else(|| ParseError::new(number, "expected nested group: [dimension=value,...]:"))?;
    let dimension = identifier(dimension.trim(), number, "source dimension")?;
    if parent.binds(dimension) {
        return Err(ParseError::new(
            number,
            format!("nested group repeats dimension `{dimension}`"),
        ));
    }
    let mut expanded = Vec::new();
    for value in comma_items(values, number)? {
        let one = parse_bindings(&format!("{dimension}={value}]"), number)?;
        let mut combined = parent.clone();
        combined.extend(&one);
        if expanded.contains(&combined) {
            return Err(ParseError::new(
                number,
                format!("duplicate nested value `{value}`"),
            ));
        }
        expanded.push(combined);
    }
    if expanded.is_empty() {
        return Err(ParseError::new(number, "nested group needs a value"));
    }
    Ok(expanded)
}

/// Locate valid source records in an inventory without parsing each line as
/// a separate document. Used only to place diagnostics after a failed check.
pub(crate) fn source_record_lines(text: &str) -> Vec<(usize, SourceRecord)> {
    let layout = Layout::new(text);
    let parsed = read_inventory(layout.numbered().filter(|(_, line)| line.record));
    if parsed.errors.is_empty() {
        parsed.record_lines
    } else {
        Vec::new()
    }
}

fn parse_context(line: &str, number: usize) -> Result<EntityBinding, ParseError> {
    let bindings = line
        .strip_prefix('[')
        .ok_or_else(|| ParseError::new(number, "expected context: [dimension=value,...]"))?;
    parse_bindings(bindings, number)
}

/// Parse `dimension=value, ...]`, the text after a record's opening `[`.
pub(super) fn parse_bindings(bindings: &str, number: usize) -> Result<EntityBinding, ParseError> {
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
    Ok(EntityBinding::from(values))
}
