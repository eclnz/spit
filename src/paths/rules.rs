//! Checks of declared path rules, before any file or job exists: the
//! pipeline compile step, and the shape of a recipe's discovery patterns.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::{ArtifactInstance, EntityBinding, Pipeline, ProductDef};
use crate::parser::SourceMap;

use super::template::{bind_path, enclosing_path, error, PathError, PathPart, PathPlaceholder};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathRule {
    Explicit(String),
    /// An explicit source rule supplied by the recipe.
    Recipe(String),
    /// The default `path:` rule written inside a stage.
    Stage {
        stage: String,
        template: String,
    },
    Default(String),
    /// A source with no rule whose inventory records each give its file.
    Inventory,
    Missing,
}

impl PathRule {
    /// The rule `product` takes: its own, else its stage's default, else
    /// the pipeline's default.
    fn for_product(pipeline: &Pipeline, product: &str) -> Self {
        if let Some(template) = pipeline.product_paths.get(product) {
            Self::Explicit(template.to_string())
        } else if let Some((stage, template)) = pipeline.stage_path_rule(product) {
            Self::Stage {
                stage: stage.to_owned(),
                template: template.to_string(),
            }
        } else if let Some(template) = &pipeline.path_template {
            Self::Default(template.to_string())
        } else {
            Self::Missing
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverageEntry {
    pub product: String,
    pub source: bool,
    pub rule: PathRule,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverage {
    pub entries: Vec<PathCoverageEntry>,
}

impl PathCoverage {
    /// Label source rules supplied by a recipe after inspecting the merged
    /// pipeline, so the listing makes each rule's origin clear.
    #[must_use]
    pub fn with_recipe_paths<'a>(mut self, products: impl IntoIterator<Item = &'a str>) -> Self {
        let products: BTreeSet<_> = products.into_iter().collect();
        for entry in &mut self.entries {
            if entry.source && products.contains(entry.product.as_str()) {
                if let PathRule::Explicit(template) = &entry.rule {
                    entry.rule = PathRule::Recipe(template.clone());
                }
            }
        }
        self
    }

    /// Mark each source in `products` as having its files given by the
    /// inventory. Binding takes those files over any rule, so the source
    /// needs none.
    #[must_use]
    pub fn with_inventory_paths<'a>(mut self, products: impl IntoIterator<Item = &'a str>) -> Self {
        let products: BTreeSet<_> = products.into_iter().collect();
        for entry in &mut self.entries {
            if entry.source && products.contains(entry.product.as_str()) {
                entry.rule = PathRule::Inventory;
            }
        }
        self
    }

    /// Missing rules always fail. Strict mode also rejects default fallbacks.
    pub fn validate(&self, strict: bool) -> Result<(), PathError> {
        let missing: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.rule == PathRule::Missing)
            .map(|entry| entry.product.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(error(format!(
                "no path rule for products: {}",
                missing.join(", ")
            )));
        }
        if strict {
            let fallback: Vec<_> = self
                .entries
                .iter()
                .filter(|entry| matches!(entry.rule, PathRule::Default(_) | PathRule::Stage { .. }))
                .map(|entry| entry.product.as_str())
                .collect();
            if !fallback.is_empty() {
                return Err(error(format!(
                    "strict paths requires explicit rules for products: {}",
                    fallback.join(", ")
                )));
            }
        }
        Ok(())
    }
}

impl fmt::Display for PathCoverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Product path coverage:")?;
        for entry in &self.entries {
            let role = if entry.source { "source" } else { "output" };
            match &entry.rule {
                PathRule::Explicit(template) => {
                    writeln!(f, "  {} ({role}): explicit {template}", entry.product)?;
                }
                PathRule::Recipe(template) => {
                    writeln!(
                        f,
                        "  {} ({role}): explicit {template} (recipe)",
                        entry.product
                    )?;
                }
                PathRule::Stage { stage, template } => {
                    writeln!(
                        f,
                        "  {} ({role}): stage {stage} default {template}",
                        entry.product
                    )?;
                }
                PathRule::Default(template) => {
                    writeln!(f, "  {} ({role}): default {template}", entry.product)?;
                }
                PathRule::Inventory => {
                    writeln!(f, "  {} ({role}): from the inventory", entry.product)?;
                }
                PathRule::Missing => {
                    if entry.source {
                        writeln!(
                            f,
                            "  {} ({role}): no rule (a recipe may supply one)",
                            entry.product
                        )?;
                    } else {
                        writeln!(f, "  {} ({role}): MISSING", entry.product)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Inspect every declared product, including families with no resolved jobs.
pub fn inspect_paths(pipeline: &Pipeline) -> Result<PathCoverage, PathError> {
    let (coverage, errors) = collect_paths(pipeline, &SourceMap::default(), &BTreeSet::new());
    match errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(coverage),
    }
}

/// Inspect every path rule, collecting each error. Rules for products in
/// `skip` belong to declarations that already failed and are not checked.
pub(crate) fn collect_paths(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> (PathCoverage, Vec<PathError>) {
    let mut errors = Vec::new();
    for name in pipeline.product_paths.keys() {
        if !skip.contains(name)
            && !pipeline
                .products
                .iter()
                .any(|product| &product.name == name)
        {
            errors.push(
                error(format!("path refers to unknown product `{name}`"))
                    .at(lines.paths.get(name).cloned()),
            );
        }
    }
    let outputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| invocation.outputs.iter().map(String::as_str))
        .collect();
    let mut entries = Vec::new();
    let mut samples: BTreeMap<String, &str> = BTreeMap::new();
    for product in &pipeline.products {
        let rule = PathRule::for_product(pipeline, &product.name);
        if rule != PathRule::Missing && !skip.contains(&product.name) {
            let line = lines.path_rule(pipeline, &product.name);
            match validate_path_template(pipeline, product) {
                Err(e) => errors.push(e.at(line)),
                // A repeated product name is reported by the resolver as a duplicate.
                Ok(sample) => {
                    if let Some(other) = samples
                        .insert(sample.clone(), &product.name)
                        .filter(|other| *other != product.name)
                    {
                        errors.push(
                            error(format!(
                                "products `{other}` and `{}` bind to the same path `{sample}` for the same entities; include `{{product}}` or distinguish their path rules",
                                product.name
                            ))
                            .at(line),
                        );
                    }
                }
            }
        }
        entries.push(PathCoverageEntry {
            product: product.name.clone(),
            source: !outputs.contains(product.name.as_str()),
            rule,
        });
    }
    for (path, product) in &samples {
        if let Some((directory, other)) = enclosing_path(&samples, path) {
            errors.push(
                error(format!(
                    "path rule for `{product}` puts files inside `{directory}`, the path of a `{other}` file, for the same entities; distinguish their path rules"
                ))
                .at(lines.path_rule(pipeline, product)),
            );
        }
    }
    (PathCoverage { entries }, errors)
}

/// Bind a product's path rule to placeholder entities, rejecting rules that
/// cannot tell the product's artifacts apart. Returns the sample path.
fn validate_path_template(pipeline: &Pipeline, product: &ProductDef) -> Result<String, PathError> {
    let template = pipeline
        .path_template_for(&product.name)
        .ok_or_else(|| error(format!("no path template for product `{}`", product.name)))?;
    let placeholders: BTreeSet<_> = template
        .parts()
        .iter()
        .filter_map(|part| match part {
            PathPart::Placeholder(placeholder) => Some(placeholder),
            PathPart::Literal(_) => None,
        })
        .collect();
    if !placeholders.contains(&PathPlaceholder::Entities) {
        if let Some(dimension) = product.dimensions.iter().find(|dimension| {
            !placeholders.contains(&PathPlaceholder::Dimension((*dimension).clone()))
        }) {
            return Err(error(format!(
                "path template for `{}` omits dimension `{dimension}`; artifacts differing only in `{dimension}` would share a path",
                product.name
            )));
        }
    }
    // Each dimension gets a distinct sample value so that templates naming
    // different dimensions are not mistaken for colliding ones.
    let entities: EntityBinding = product
        .dimensions
        .iter()
        .map(|dimension| (dimension.clone(), dimension.clone()))
        .collect();
    let artifact = ArtifactInstance::new(&product.name, product.artifact_type.clone(), entities);
    bind_path(pipeline, &product.dimensions, artifact.view(), || {
        format!("path rule for `{}`", product.name)
    })
}
