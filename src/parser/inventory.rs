//! Source inventories: `sources:` records and `contexts:`, whether in a
//! `.spitout` or written in a `.spitin` recipe.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::{EntityBinding, InputRules, Pipeline, Removal, SourceInventory, SourceRecord};
use crate::paths::PathTemplate;

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
            Some(_) => {
                inventory_section = true;
                inventory_line.get_or_insert(index + 1);
            }
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
    InventoryText {
        inventory,
        pipeline,
        rules,
    }
    .to_string()
}

/// Settled `inventory` as writing it as a `.spitout` for `rules` and reading that back gives it, but in its own order, or `None`
/// when a path rule it would write holds a `#`. Settling gives each record
/// the path its rule gives, which the text leaves out.
///
/// Keep in step with `InventoryText`, which writes the text, and
/// `parse_inventory_with_lines`, which reads it: this must do to the
/// inventory whatever the two do to it together, or a recipe diagnosed in
/// memory will differ from its text. `tests/outputs.rs` compares them.
pub(crate) fn as_read_back(
    inventory: &SourceInventory,
    rules: &InputRules,
) -> Option<SourceInventory> {
    let source_paths = written_source_paths(inventory, rules);
    if source_paths
        .values()
        .any(|template| template.to_string().contains('#'))
    {
        return None;
    }
    let mut read = inventory.clone();
    read.source_paths = source_paths;
    for record in &mut read.artifacts {
        record.path = None;
    }
    // Every named context is written as a context, and contexts are read
    // back sorted, each once.
    read.contexts
        .extend(read.discovered.values().flatten().cloned());
    read.contexts.sort();
    read.contexts.dedup();
    for bindings in read.discovered.values_mut() {
        bindings.sort();
        bindings.dedup();
    }
    Some(read)
}

/// The source path rules a `.spitout` writes: the inventory's and the
/// recipe's.
fn written_source_paths(
    inventory: &SourceInventory,
    rules: &InputRules,
) -> BTreeMap<String, PathTemplate> {
    let mut source_paths = inventory.source_paths.clone();
    source_paths.extend(rules.source_paths.clone());
    source_paths
}

/// A `.spitout`: its contexts, unnamed then by discovery rule, and its
/// source records.
///
/// Keep in step with `as_read_back`: whatever this leaves out or rewrites,
/// such as a record's path or the order of contexts, it must too.
struct InventoryText<'a> {
    inventory: &'a SourceInventory,
    pipeline: &'a Pipeline,
    rules: &'a InputRules,
}

/// The records a `.spitout` writes under a discovered context instead of
/// under `sources:`: for each context, each remaining dimension's value (or
/// none) and the products with a record there.
type Nested<'a> = FxHashMap<EntityBinding, FxHashMap<EntityBinding, Vec<&'a str>>>;

impl fmt::Display for InventoryText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inventory = self.inventory;
        if let Some(root) = &inventory.root {
            writeln!(f, "root {}", root.display())?;
            writeln!(f)?;
        }
        let source_paths = written_source_paths(inventory, self.rules);
        if !source_paths.is_empty() {
            writeln!(f, "source_paths:")?;
            for (name, template) in &source_paths {
                writeln!(f, "    {name}: {template}")?;
            }
            writeln!(f)?;
        }
        let (nested, flat) = self.nest_records();
        if !flat.is_empty() {
            writeln!(f, "sources:")?;
            for record in flat {
                self.write_record(f, record)?;
            }
            if !inventory.contexts.is_empty() {
                writeln!(f)?;
            }
        }
        let named: FxHashSet<_> = inventory.discovered.values().flatten().collect();
        let unnamed: Vec<_> = inventory
            .contexts
            .iter()
            .filter(|context| !named.contains(context))
            .collect();
        if !unnamed.is_empty() {
            writeln!(f, "contexts:")?;
            self.write_contexts(f, unnamed, &[])?;
            writeln!(f)?;
        }
        for (name, bindings) in &inventory.discovered {
            writeln!(f, "contexts {name}:")?;
            let declared = self
                .rules
                .discovery(name)
                .map_or(&[][..], |rule| rule.dimensions.as_slice());
            let mut bindings: Vec<_> = bindings.iter().collect();
            bindings.sort_by(|left, right| left.cmp_in(right, declared));
            let order: Vec<_> = declared.iter().map(String::as_str).collect();
            for (index, binding) in bindings.into_iter().enumerate() {
                if index > 0 {
                    writeln!(f)?;
                }
                let groups = nested.get(binding);
                let colon = if groups.is_some() { ":" } else { "" };
                writeln!(f, "    [{}]{colon}", in_order(binding, &order))?;
                if let Some(groups) = groups {
                    self.write_groups(f, groups)?;
                }
            }
        }
        self.write_removed(f)
    }
}

impl InventoryText<'_> {
    /// What the input stage removed, each with the rule that removed it:
    ///
    /// ```text
    /// removed:
    ///     bold[sub=02,ses=02,run=3]
    ///         rule: exclude bold[run=3,ses=02,sub=02]
    ///         at: line 4
    ///         reason: corrupted
    /// ```
    fn write_removed(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let removed = &self.inventory.removed;
        if removed.is_empty() {
            return Ok(());
        }
        let order = self.pipeline_order();
        let order: Vec<_> = order.iter().map(String::as_str).collect();
        writeln!(f, "\nremoved:")?;
        for removal in removed {
            let declared: Vec<&str> = removal
                .product
                .as_deref()
                .and_then(|name| {
                    self.pipeline
                        .products
                        .iter()
                        .find(|product| product.name == name)
                })
                .map_or_else(
                    || order.clone(),
                    |product| product.dimensions.iter().map(String::as_str).collect(),
                );
            let product = removal.product.as_deref().unwrap_or_default();
            writeln!(
                f,
                "    {product}[{}]",
                in_order(&removal.entities, &declared)
            )?;
            writeln!(f, "        rule: {}", removal.rule)?;
            if let Some(origin) = &removal.origin {
                writeln!(f, "        at: {origin}")?;
            }
            if let Some(found) = removal.found {
                writeln!(f, "        found: {found}")?;
            }
            if let Some(reason) = &removal.reason {
                writeln!(f, "        reason: {reason}")?;
            }
        }
        Ok(())
    }

    /// The pipeline's dimensions, each once, in the order its products
    /// declare them.
    fn pipeline_order(&self) -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for dimension in self
            .pipeline
            .products
            .iter()
            .flat_map(|product| &product.dimensions)
        {
            if !order.contains(dimension) {
                order.push(dimension.clone());
            }
        }
        order
    }

    /// Split the records into those written under their discovered context
    /// and those written under `sources:`. A single named discovery gives
    /// each record with its dimensions one unambiguous context, and at most
    /// one remaining dimension can be grouped under it.
    fn nest_records(&self) -> (Nested<'_>, Vec<&SourceRecord>) {
        let inventory = self.inventory;
        let discovery = match inventory.discovered.iter().next() {
            Some((name, contexts)) if inventory.discovered.len() == 1 => self
                .rules
                .discovery(name)
                .map(|rule| (contexts.iter().collect::<FxHashSet<_>>(), &rule.dimensions)),
            _ => None,
        };
        let mut nested: Nested<'_> = FxHashMap::default();
        let mut flat = Vec::new();
        for record in &inventory.artifacts {
            let under = discovery.as_ref().and_then(|(contexts, dimensions)| {
                let parent = record.entities.project(dimensions)?;
                let remainder = record.entities.except(dimensions);
                (contexts.contains(&parent) && remainder.len() <= 1).then_some((parent, remainder))
            });
            match under {
                Some((parent, remainder)) => nested
                    .entry(parent)
                    .or_default()
                    .entry(remainder)
                    .or_default()
                    .push(&record.product),
                None => flat.push(record),
            }
        }
        (nested, flat)
    }

    /// A record under `sources:`; its path rule gives its file.
    fn write_record(&self, f: &mut fmt::Formatter<'_>, record: &SourceRecord) -> fmt::Result {
        if record.entities.is_empty() {
            write!(f, "    {}", record.product)?;
        } else {
            let declared: Vec<_> = self
                .pipeline
                .products
                .iter()
                .find(|product| product.name == record.product)
                .map_or_else(Vec::new, |product| {
                    product.dimensions.iter().map(String::as_str).collect()
                });
            let entities = in_order(&record.entities, &declared);
            write!(f, "    {}[{entities}]", record.product)?;
        }
        writeln!(f)
    }

    /// Each of `contexts` on its own line, ordered and written by `first`'s
    /// dimensions, then the pipeline's in the order its products name them.
    fn write_contexts(
        &self,
        f: &mut fmt::Formatter<'_>,
        mut contexts: Vec<&EntityBinding>,
        first: &[String],
    ) -> fmt::Result {
        let mut order = first.to_vec();
        for dimension in self.pipeline_order() {
            if !order.contains(&dimension) {
                order.push(dimension);
            }
        }
        contexts.sort_by(|left, right| left.cmp_in(right, &order));
        let order: Vec<&str> = order.iter().map(String::as_str).collect();
        for context in contexts {
            writeln!(f, "    [{}]", in_order(context, &order))?;
        }
        Ok(())
    }

    /// The products under one context: those with no further dimension, then
    /// each further value, with runs of values that list the same products
    /// written as one group, such as `[run=1,2]:`.
    fn write_groups(
        &self,
        f: &mut fmt::Formatter<'_>,
        groups: &FxHashMap<EntityBinding, Vec<&str>>,
    ) -> fmt::Result {
        let products = &self.pipeline.products;
        let sorted_names = |names: &[&str]| {
            let mut names = names.to_vec();
            names.sort_by_key(|name| {
                products
                    .iter()
                    .position(|product| product.name == *name)
                    .unwrap_or(usize::MAX)
            });
            names.join(", ")
        };
        if let Some(names) = groups.get(&EntityBinding::default()) {
            writeln!(f, "        {}", sorted_names(names))?;
        }
        let pipeline_order = self.pipeline_order();
        let mut remaining: Vec<_> = groups
            .iter()
            .filter(|(binding, _)| !binding.is_empty())
            .collect();
        remaining.sort_by(|(left, _), (right, _)| left.cmp_in(right, &pipeline_order));
        let mut remaining = remaining
            .into_iter()
            .filter_map(|(binding, names)| {
                let (dimension, value) = binding.iter().next()?;
                Some((dimension, value, sorted_names(names)))
            })
            .peekable();
        while let Some((dimension, value, names)) = remaining.next() {
            let mut values = vec![value];
            while let Some((_, next, _)) = remaining.next_if(|(next_dimension, _, next_names)| {
                *next_dimension == dimension && *next_names == names
            }) {
                values.push(next);
            }
            writeln!(f, "        [{dimension}={}]:", values.join(","))?;
            writeln!(f, "            {names}")?;
        }
        Ok(())
    }
}

/// `binding` as `dim=value,...`, in the order of `declared`, then any
/// dimension it does not name.
fn in_order(binding: &EntityBinding, declared: &[&str]) -> String {
    let mut values: Vec<_> = binding.iter().collect();
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

/// Parse an inventory supplied by a dataset indexer or written as a fixture.
/// Records are logical identities, each optionally followed by `: path`, the
/// file relative to the dataset root. They never hold artifact types.
pub fn parse_source_inventory(text: &str) -> Result<SourceInventory, ParseError> {
    parse_inventory_with_lines(text).map(|(inventory, _)| inventory)
}

fn parse_inventory_with_lines(
    text: &str,
) -> Result<(SourceInventory, Vec<(usize, SourceRecord)>), ParseError> {
    enum InventorySection {
        Sources,
        SourcePaths,
        Contexts(Option<String>),
        Removed,
    }

    let text = super::without_bom(text);
    let mut inventory = SourceInventory::default();
    let mut record_lines = Vec::new();
    let mut section = None;
    let mut parent: Option<(EntityBinding, usize)> = None;
    let mut group: Option<(Vec<EntityBinding>, usize)> = None;
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
        let indent = original.len() - original.trim_start().len();
        if line.is_empty() {
            continue;
        }
        if let Some(folder) = line.strip_prefix("root ") {
            if section.is_some() {
                return Err(ParseError::new(
                    number,
                    "`root` comes before every section of a .spitout",
                )
                .locate(original));
            }
            if inventory.root.is_some() {
                return Err(
                    ParseError::new(number, "a .spitout names its root once").locate(original)
                );
            }
            inventory.root = Some(PathBuf::from(folder.trim()));
            continue;
        }
        match Header::of(line) {
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
                section = Some(InventorySection::Contexts(None));
                parent = None;
                group = None;
            }
            Some(Header::Removed) => {
                section = Some(InventorySection::Removed);
                parent = None;
                group = None;
            }
            Some(Header::Contexts(Some(name))) => {
                let name = identifier(name, number, "discovery name")
                    .map_err(|error| error.locate(original))?;
                // A named section says the discovery ran, even when every
                // context it found was skipped and the section is empty.
                inventory.discovered.entry(name.to_owned()).or_default();
                section = Some(InventorySection::Contexts(Some(name.to_owned())));
                parent = None;
                group = None;
            }
            _ => match section {
                Some(InventorySection::Sources) => {
                    if line.contains(['[', ']', '=']) {
                        let record = parse_source(line, number).map_err(|e| e.locate(original))?;
                        record_lines.push((number, record.clone()));
                        inventory.artifacts.push(record);
                    } else {
                        let records = parse_product_list(line, number, &EntityBinding::default())
                            .map_err(|e| e.locate(original))?;
                        record_lines.extend(records.iter().cloned().map(|record| (number, record)));
                        inventory.artifacts.extend(records);
                    }
                }
                Some(InventorySection::Removed) => {
                    parse_removed_line(original, number, &mut inventory.removed)
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
                    if inventory
                        .source_paths
                        .insert(name.to_owned(), template)
                        .is_some()
                    {
                        return Err(ParseError::new(
                            number,
                            format!("duplicate source path rule for `{name}`"),
                        )
                        .locate(original));
                    }
                }
                Some(InventorySection::Contexts(ref name)) => {
                    let nests_under = parent
                        .as_ref()
                        .filter(|(_, at)| line.starts_with('[') && indent > *at);
                    if let Some((context, _)) = nests_under {
                        let values =
                            parse_group(line, number, context).map_err(|e| e.locate(original))?;
                        group = Some((values, indent));
                    } else if line.starts_with('[') {
                        let context = parse_context(line.trim_end_matches(':'), number)
                            .map_err(|e| e.locate(original))?;
                        inventory.contexts.push(context.clone());
                        if let Some(name) = name {
                            inventory
                                .discovered
                                .entry(name.clone())
                                .or_default()
                                .push(context.clone());
                        }
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
                            .map_or_else(|| vec![context.clone()], |(values, _)| values.clone());
                        for base in bases {
                            let records = parse_product_list(line, number, &base)
                                .map_err(|e| e.locate(original))?;
                            record_lines
                                .extend(records.iter().cloned().map(|record| (number, record)));
                            inventory.artifacts.extend(records);
                        }
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
    // Keep in step with `as_read_back`, which must normalize a settled
    // inventory as reading its text does here.
    inventory.contexts.sort();
    inventory.contexts.dedup();
    for bindings in inventory.discovered.values_mut() {
        bindings.sort();
        bindings.dedup();
    }
    Ok((inventory, record_lines))
}

/// Read one line of a `removed:` section into `removed`: what was removed,
/// `product[dimension=value,...]` or `[dimension=value,...]`, or one of its
/// `rule:`, `at:` and `reason:` lines. Values are read from the whole line,
/// so a `#` in a reason is kept.
fn parse_removed_line(
    original: &str,
    number: usize,
    removed: &mut Vec<Removal>,
) -> Result<(), ParseError> {
    let line = original.trim();
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
    let line = strip_comment(original).trim();
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
    let inventory_text = split_document(text).inventory;
    parse_inventory_with_lines(&inventory_text).map_or_else(|_| Vec::new(), |(_, lines)| lines)
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
