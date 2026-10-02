//! Writing a source inventory as a `.spitout`, in the form
//! `parse_source_inventory` reads back.

use std::collections::BTreeMap;
use std::fmt;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::{EntityBinding, InputRules, Pipeline, SourceInventory, SourceRecord};
use crate::paths::PathTemplate;

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
