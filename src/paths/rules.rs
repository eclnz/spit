//! Checks of declared path rules, before any file or job exists: the
//! pipeline compile step, and the shape of a recipe's discovery patterns.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::{
    ArtifactInstance, EntityBinding, ExtensionSource, Pipeline, PipelineIndex, ProductDef,
};
use crate::parser::SourceMap;

use super::template::{
    bind_path, enclosing_path, error, shown_path, PathError, PathPart, PathPlaceholder,
    PathTemplate,
};

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
    /// An output its tool writes beside another's file, whose path follows
    /// that file's.
    Beside {
        sibling: String,
        template: String,
    },
    /// A source with no rule whose inventory records each give its file.
    Inventory,
    Missing,
}

impl PathRule {
    /// The rule `product` takes: its own, else its stage's default, else
    /// the pipeline's default, each with any extension added to it.
    fn for_product(index: &PipelineIndex<'_>, product: &str) -> Self {
        let Some(template) = index.path_template_for(product) else {
            return Self::Missing;
        };
        let beside = index.beside(product);
        let rule_product = beside.map_or(product, |(sibling, _, _)| sibling);
        let template = if index
            .path_rule_for(rule_product)
            .is_some_and(PathTemplate::varies)
        {
            shown_path(index, product).unwrap_or_else(|| template.to_string())
        } else {
            template.to_string()
        };
        if let Some((sibling, _, _)) = beside {
            Self::Beside {
                sibling: sibling.to_owned(),
                template,
            }
        } else if index.pipeline.product_paths.contains_key(product) {
            Self::Explicit(template)
        } else if let Some((stage, _)) = index.stage_path_rule(product) {
            Self::Stage {
                stage: stage.to_owned(),
                template,
            }
        } else {
            Self::Default(template)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverageEntry {
    pub product: String,
    pub source: bool,
    pub rule: PathRule,
    /// The extension added to the rule as written, and where it is
    /// declared: `` `.mat` from operation `align` ``.
    pub extension: Option<String>,
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
            write!(f, "  {} ({role}): ", entry.product)?;
            match &entry.rule {
                PathRule::Explicit(template) => write!(f, "explicit {template}")?,
                PathRule::Recipe(template) => write!(f, "explicit {template} (recipe)")?,
                PathRule::Stage { stage, template } => {
                    write!(f, "stage {stage} default {template}")?;
                }
                PathRule::Default(template) => write!(f, "default {template}")?,
                PathRule::Beside { sibling, template } => {
                    write!(f, "beside {sibling} {template}")?;
                }
                PathRule::Inventory => write!(f, "from the inventory")?,
                PathRule::Missing if entry.source => {
                    write!(f, "no rule (a recipe may supply one)")?
                }
                PathRule::Missing => write!(f, "MISSING")?,
            }
            match &entry.extension {
                Some(extension) => writeln!(f, ", {extension}")?,
                None => writeln!(f)?,
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
    let index = PipelineIndex::new(pipeline);
    let mut errors = Vec::new();
    for name in pipeline.product_paths.keys() {
        if !skip.contains(name) && index.product(name).is_none() {
            errors.push(
                error(format!("path refers to unknown product `{name}`"))
                    .at(lines.paths.get(name).cloned()),
            );
        }
    }
    // A group drops a dimension a product lacks, so one no product has,
    // usually a typo, would be dropped silently.
    let had: BTreeSet<_> = pipeline
        .products
        .iter()
        .flat_map(|product| product.dimensions.iter().map(String::as_str))
        .collect();
    let rules = pipeline
        .path_template
        .iter()
        .map(|template| (template, lines.default_path.clone()))
        .chain(pipeline.stages.iter().filter_map(|stage| {
            Some((
                stage.path_template.as_ref()?,
                lines.stage_paths.get(&stage.name).cloned(),
            ))
        }))
        .chain(
            pipeline
                .product_paths
                .iter()
                .filter(|(product, _)| !skip.contains(*product))
                .map(|(product, template)| (template, lines.paths.get(product).cloned())),
        );
    for (template, line) in rules {
        let unknown: BTreeSet<_> = template
            .group_dimensions()
            .filter(|dimension| !had.contains(dimension))
            .collect();
        for dimension in unknown {
            errors.push(
                error(format!(
                    "path rule names `{{{dimension}}}`, which no product has"
                ))
                .at(line.clone())
                .focus(format!("{{{dimension}}}")),
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
    // One default rule can disagree with many products' extensions; each
    // disagreement is said once.
    let mut disagreements = BTreeSet::new();
    for product in &pipeline.products {
        let rule = PathRule::for_product(&index, &product.name);
        if rule != PathRule::Missing && !skip.contains(&product.name) {
            let line = lines.path_rule(&index, &product.name);
            if let Some((sibling, _, _)) = index.beside(&product.name) {
                if pipeline.product_paths.contains_key(&product.name) {
                    errors.push(
                        error(format!(
                            "`{}` is written beside `{sibling}`, so its path follows `{sibling}`'s; remove its path rule",
                            product.name
                        ))
                        .at(lines.paths.get(&product.name).cloned()),
                    );
                }
            }
            if let Some(problem) = extension_disagreement(&index, &product.name) {
                if disagreements.insert(problem.clone()) {
                    errors.push(error(problem).at(line.clone()));
                }
            }
            match validate_path_template(&index, product) {
                Err(e) => errors.push(e.at(line)),
                // A repeated product name is reported by the resolver as a duplicate.
                Ok(sample) => {
                    if let Some(other) = samples
                        .insert(sample.clone(), &product.name)
                        .filter(|other| *other != product.name)
                    {
                        errors.push(
                            error(format!(
                                "products `{other}` and `{}` bind to the same path `{sample}` for the same entities; include `{{@product}}` or distinguish their path rules",
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
            extension: index
                .added_extension(&product.name)
                .map(|(extension, source)| format!("`{extension}` from {source}")),
        });
    }
    for (path, product) in &samples {
        if let Some((directory, other)) = enclosing_path(&samples, path) {
            errors.push(
                error(format!(
                    "path rule for `{product}` puts files inside `{directory}`, the path of a `{other}` file, for the same entities; distinguish their path rules"
                ))
                .at(lines.path_rule(&index, product)),
            );
        }
    }
    (PathCoverage { entries }, errors)
}

/// Why `product`'s path rule ends with an extension other than the one its
/// file must have, if it does. A rule that ends with none is given it.
fn extension_disagreement(index: &PipelineIndex<'_>, product: &str) -> Option<String> {
    let (expected, source) = index.expected_extension(product)?;
    let rule = index.path_rule_for(product)?;
    // A `.` earlier in the file name is not part of its extension.
    if rule.ends_with(expected) {
        return None;
    }
    let written = rule.extension()?;
    if index.pipeline.product_paths.contains_key(product) {
        return Some(format!(
            "path `{product}` ends in `{written}`, but {source} writes `{expected}`; drop the extension or use `{expected}`"
        ));
    }
    let default = match index.stage_path_rule(product) {
        Some((stage, _)) => format!("stage `{stage}`'s default path"),
        None => "the default path".to_owned(),
    };
    Some(match source {
        ExtensionSource::Operation(_) => format!(
            "{default} ends in `{written}`, but {source} writes `{expected}`; write {default} without an extension, and give the outputs that use it `ext: {written}`"
        ),
        ExtensionSource::Stage(_) | ExtensionSource::Default => format!(
            "{default} ends in `{written}`, but {source} sets `{expected}`; write the extension once, with `ext:`"
        ),
    })
}

/// Bind a product's path rule to placeholder entities, rejecting rules that
/// cannot tell the product's artifacts apart. Returns the sample path.
fn validate_path_template(
    index: &PipelineIndex<'_>,
    product: &ProductDef,
) -> Result<String, PathError> {
    let template = index
        .path_template_for(&product.name)
        .ok_or_else(|| error(format!("no path template for product `{}`", product.name)))?;
    let placeholders: BTreeSet<_> = template
        .parts()
        .iter()
        .filter_map(|part| match part {
            PathPart::Placeholder(placeholder) => Some(placeholder),
            PathPart::Literal(_) | PathPart::Group(_) => None,
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
    bind_path(index, &product.dimensions, artifact.view(), || {
        format!("path rule for `{}`", product.name)
    })
}
