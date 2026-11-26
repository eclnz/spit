use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::{
    DimensionOrders, EntityBinding, InputRules, Pipeline, SourceInventory, SourceRecord,
};
use crate::template::PathTemplate;

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
    let projection = Projection::new(inventory, pipeline, rules);
    if !projection.reads_back() {
        return None;
    }
    let mut read = inventory.clone();
    for record in &mut read.artifacts {
        if !projection.keeps_path(record) {
            record.path = None;
        }
    }
    read.source_paths = projection.source_paths;
    normalize_contexts(&mut read.contexts);
    Some(read)
}

pub(crate) fn normalize_contexts<T: Ord>(contexts: &mut Vec<T>) {
    contexts.sort();
    contexts.dedup();
}

struct Projection {
    source_paths: BTreeMap<String, PathTemplate>,
    reading: BTreeSet<String>,
}

impl Projection {
    fn new(inventory: &SourceInventory, pipeline: &Pipeline, rules: &InputRules) -> Self {
        let source_paths = written_source_paths(inventory, pipeline, rules);
        let reading = reading_sources(pipeline, &source_paths);
        Self {
            source_paths,
            reading,
        }
    }

    fn reads_back(&self) -> bool {
        !self
            .source_paths
            .values()
            .any(|template| template.to_string().contains('#'))
    }

    fn keeps_path(&self, record: &SourceRecord) -> bool {
        self.reading.contains(record.product.as_str())
    }
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
        let projection = Projection::new(inventory, self.pipeline, self.rules);
        if !projection.source_paths.is_empty() {
            writeln!(f, "source_paths:")?;
            for (name, template) in &projection.source_paths {
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
        if !inventory.artifacts.is_empty() {
            writeln!(f, "sources:")?;
            for record in &inventory.artifacts {
                self.write_record(f, record, projection.keeps_path(record))?;
            }
            if !inventory.contexts.is_empty() {
                writeln!(f)?;
            }
        }
        if !inventory.contexts.is_empty() {
            writeln!(f, "contexts:")?;
            let mut contexts: Vec<_> = inventory.contexts.iter().collect();
            normalize_contexts(&mut contexts);
            self.write_contexts(f, contexts, &[])?;
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
        let files = &self.inventory.review_files;
        if files.is_empty() {
            return Ok(());
        }
        writeln!(f, "\nverdicts:")?;
        for file in files {
            writeln!(f, "    {} from {}", file.review, file.file.display())?;
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
