//! Writing a source inventory as a `.spitout`, in the form
//! `parse_source_inventory` reads back.

use std::collections::BTreeMap;
use std::fmt;

use crate::model::{
    DimensionOrders, EntityBinding, InputRules, Pipeline, SourceInventory, SourceRecord,
};
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
    pipeline: &Pipeline,
    rules: &InputRules,
) -> Option<SourceInventory> {
    let source_paths = written_source_paths(inventory, pipeline, rules);
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
    // Contexts are written without discovery names and read back sorted.
    read.contexts.sort();
    read.contexts.dedup();
    Some(read)
}

/// The source path rules a `.spitout` writes: the inventory's and the
/// recipe's. Companions derive their paths from their main source.
fn written_source_paths(
    inventory: &SourceInventory,
    pipeline: &Pipeline,
    rules: &InputRules,
) -> BTreeMap<String, PathTemplate> {
    let mut source_paths = inventory.source_paths.clone();
    source_paths.extend(rules.named_source_paths(pipeline).into_owned());
    if pipeline.entities_format.is_some() {
        let index = crate::model::PipelineIndex::new(pipeline);
        for (product, template) in &mut source_paths {
            *template = template.resolve(index.holder(product)).into_owned();
        }
    }
    source_paths
}

/// A `.spitout`: its observed contexts and source records.
///
/// Keep in step with `as_read_back`: whatever this leaves out or rewrites,
/// such as a record's path or the order of contexts, it must too.
struct InventoryText<'a> {
    inventory: &'a SourceInventory,
    pipeline: &'a Pipeline,
    rules: &'a InputRules,
}

impl fmt::Display for InventoryText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inventory = self.inventory;
        if let Some(root) = &inventory.root {
            writeln!(f, "root {}", root.display())?;
            writeln!(f)?;
        }
        let source_paths = written_source_paths(inventory, self.pipeline, self.rules);
        if !source_paths.is_empty() {
            writeln!(f, "source_paths:")?;
            for (name, template) in &source_paths {
                writeln!(f, "    {name}: {template}")?;
            }
            writeln!(f)?;
        }
        if !inventory.artifacts.is_empty() {
            writeln!(f, "sources:")?;
            for record in &inventory.artifacts {
                self.write_record(f, record)?;
            }
            if !inventory.contexts.is_empty() {
                writeln!(f)?;
            }
        }
        if !inventory.contexts.is_empty() {
            writeln!(f, "contexts:")?;
            self.write_contexts(f, inventory.contexts.iter().collect(), &[])?;
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
        let orders = DimensionOrders::new(self.pipeline);
        writeln!(f, "\nremoved:")?;
        for removal in removed {
            let declared = orders.of(removal);
            let product = removal.product.as_deref().unwrap_or_default();
            writeln!(
                f,
                "    {product}[{}]",
                in_order(&removal.entities, declared)
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
        for dimension in self.pipeline.dimension_order() {
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
}

/// `binding` as `dim=value,...`, in the order of `declared`, then any
/// dimension it does not name.
fn in_order(binding: &EntityBinding, declared: &[impl AsRef<str>]) -> String {
    let mut values: Vec<_> = binding.iter().collect();
    values.sort_by_key(|(dimension, _)| {
        declared
            .iter()
            .position(|declared| declared.as_ref() == *dimension)
            .unwrap_or(usize::MAX)
    });
    values
        .into_iter()
        .map(|(dimension, value)| format!("{dimension}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}
