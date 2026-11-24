use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::{
    DimensionOrders, EntityBinding, InputRules, Pipeline, SourceInventory, SourceRecord,
};
use crate::template::PathTemplate;

use super::inventory::MISSING;

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
    let reading = reading_sources(pipeline, &source_paths);
    read.source_paths = source_paths;
    for record in &mut read.artifacts {
        if !reading.contains(record.product.as_str()) {
            record.path = None;
        }
    }
    read.contexts.sort();
    read.contexts.dedup();
    Some(read)
}

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

fn reading_sources(
    pipeline: &Pipeline,
    source_paths: &BTreeMap<String, PathTemplate>,
) -> BTreeSet<String> {
    let index = crate::model::PipelineIndex::new(pipeline);
    pipeline
        .products
        .iter()
        .filter(|product| {
            source_paths
                .get(&product.name)
                .or_else(|| index.path_rule_for(&product.name))
                .is_some_and(|template| !template.writes_paths())
        })
        .map(|product| product.name.clone())
        .collect()
}

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
        }
        if let Some(output) = &inventory.output {
            writeln!(f, "output {}", output.display())?;
        }
        if inventory.root.is_some() || inventory.output.is_some() {
            writeln!(f)?;
        }
        let source_paths = written_source_paths(inventory, self.pipeline, self.rules);
        if !source_paths.is_empty() {
            writeln!(f, "source_paths:")?;
            for (name, template) in &source_paths {
                if template.is_several() {
                    writeln!(f, "    {name}:")?;
                    for rule in template.rules() {
                        writeln!(f, "        {}", rule.as_str())?;
                    }
                } else {
                    writeln!(f, "    {name}: {template}")?;
                }
            }
            writeln!(f)?;
        }
        let reading = reading_sources(self.pipeline, &source_paths);
        if !inventory.artifacts.is_empty() {
            writeln!(f, "sources:")?;
            for record in &inventory.artifacts {
                self.write_record(f, record, reading.contains(record.product.as_str()))?;
            }
            if !inventory.contexts.is_empty() {
                writeln!(f)?;
            }
        }
        if !inventory.contexts.is_empty() {
            writeln!(f, "contexts:")?;
            self.write_contexts(f, inventory.contexts.iter().collect(), &[])?;
        }
        self.write_removed(f)?;
        self.write_verdicts(f)
    }
}

impl InventoryText<'_> {
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

    fn write_verdicts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdicts = &self.inventory.verdicts;
        let files = &self.inventory.review_files;
        if verdicts.is_empty() && files.is_empty() {
            return Ok(());
        }
        let declared = self.pipeline.dimension_order();
        writeln!(f, "\nverdicts:")?;
        for file in files {
            let missing = if file.found || verdicts.is_empty() {
                ""
            } else {
                MISSING
            };
            writeln!(
                f,
                "    {} from {}{missing}",
                file.review,
                file.file.display()
            )?;
        }
        for row in verdicts {
            writeln!(
                f,
                "    {}[{}]",
                row.review,
                in_order(&row.entities, &declared)
            )?;
            writeln!(f, "        verdict: {}", row.verdict.word())?;
            writeln!(f, "        at: {}", row.origin())?;
            if let Some(reason) = &row.reason {
                writeln!(f, "        reason: {reason}")?;
            }
        }
        Ok(())
    }

    fn write_record(
        &self,
        f: &mut fmt::Formatter<'_>,
        record: &SourceRecord,
        reads: bool,
    ) -> fmt::Result {
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
        match record.path.as_deref().filter(|_| reads) {
            Some(path) => writeln!(f, ": {path}"),
            None => writeln!(f),
        }
    }

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
