//! Checks of declared path rules, before any file or job exists: the
//! pipeline compile step, and the shape of a recipe's discovery patterns.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustc_hash::FxHashSet;

use crate::model::{
    ArtifactInstance, EntityBinding, ExtensionSource, PathOrigin, Pipeline, PipelineIndex,
    ProductDef,
};
use crate::parser::SourceMap;
use crate::span::Place;

use super::components::enclosing_path;
use super::product::{bind_path, shown_path};
use super::template::{error, PathError, PathPart, PathPlaceholder, PathTemplate};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathRule {
    Explicit(String),
    /// An explicit source rule supplied by the recipe.
    Recipe(String),
    /// The recipe's default `path:` rule, for a source with none of its own.
    RecipeDefault(String),
    /// The default `path:` rule written inside a stage.
    Stage {
        stage: String,
        template: String,
    },
    Default(String),
    /// The built-in default, for an output in a pipeline with no `path:`.
    BuiltIn(String),
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
    /// `template` is its path template, from [`PipelineIndex::path_templates`].
    fn for_product(
        index: &PipelineIndex<'_>,
        product: &str,
        template: Option<&PathTemplate>,
    ) -> Self {
        let Some(template) = template else {
            return Self::Missing;
        };
        let beside = index.beside(product);
        let rule_product = beside.map_or(product, |(sibling, _, _)| sibling);
        let template = if index
            .path_rule_for(rule_product)
            .is_some_and(PathTemplate::varies)
        {
            shown_path(index, product, template)
        } else {
            template.to_string()
        };
        match (beside, index.path_origin(product)) {
            (Some((sibling, _, _)), _) => Self::Beside {
                sibling: sibling.to_owned(),
                template,
            },
            (None, Some((PathOrigin::Explicit, _))) => Self::Explicit(template),
            (None, Some((PathOrigin::Stage(stage), _))) => Self::Stage {
                stage: stage.to_owned(),
                template,
            },
            (None, Some((PathOrigin::BuiltIn, _))) => Self::BuiltIn(template),
            (None, Some((PathOrigin::Default, _)) | None) => Self::Default(template),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverageEntry {
    pub product: String,
    pub source: bool,
    /// Whether the product's artifacts are folders.
    pub folder: bool,
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

    /// Label the sources whose rule is the recipe's default `path:`, given
    /// to them as their own after inspecting the merged pipeline.
    #[must_use]
    pub fn with_recipe_default<'a>(mut self, products: impl IntoIterator<Item = &'a str>) -> Self {
        let products: BTreeSet<_> = products.into_iter().collect();
        for entry in &mut self.entries {
            if entry.source && products.contains(entry.product.as_str()) {
                if let PathRule::Explicit(template) = &entry.rule {
                    entry.rule = PathRule::RecipeDefault(template.clone());
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

    /// Fail the products in `used` that no rule covers. Every output has a
    /// rule, if only the built-in default, so only a source can have none.
    pub fn validate<'a>(&self, used: impl IntoIterator<Item = &'a str>) -> Result<(), PathError> {
        let used: BTreeSet<_> = used.into_iter().collect();
        let missing: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.rule == PathRule::Missing)
            .map(|entry| entry.product.as_str())
            .filter(|product| used.contains(product))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let named: Vec<_> = missing
            .iter()
            .map(|product| format!("`{product}`"))
            .collect();
        let (subject, verb) = match named.len() {
            1 => ("source", "has"),
            _ => ("sources", "have"),
        };
        Err(error(format!(
            "{subject} {} {verb} no path rule, so {} files cannot be found; write `path {}:` in the pipeline, in the recipe, or under the .spitout's `source_paths:`",
            named.join(", "),
            if named.len() == 1 { "its" } else { "their" },
            missing[0]
        )))
    }
}

impl fmt::Display for PathCoverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Product path coverage:")?;
        for entry in &self.entries {
            let role = if entry.source { "source" } else { "output" };
            let kind = if entry.folder { " folder" } else { "" };
            write!(f, "  {} ({role}{kind}): ", entry.product)?;
            match &entry.rule {
                PathRule::Explicit(template) => write!(f, "explicit {template}")?,
                PathRule::Recipe(template) => write!(f, "explicit {template} (recipe)")?,
                PathRule::RecipeDefault(template) => write!(f, "default {template} (recipe)")?,
                PathRule::Stage { stage, template } => {
                    write!(f, "stage {stage} default {template}")?;
                }
                PathRule::Default(template) => write!(f, "default {template}")?,
                PathRule::BuiltIn(template) => write!(f, "built-in default {template}")?,
                PathRule::Beside { sibling, template } => {
                    write!(f, "beside {sibling} {template}")?;
                }
                PathRule::Inventory => write!(f, "from the inventory")?,
                PathRule::Missing => write!(f, "no rule (a recipe may supply one)")?,
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
    shape_errors(pipeline, lines, skip, &outputs, &mut errors);
    let mut entries = Vec::new();
    let mut samples: BTreeMap<String, &str> = BTreeMap::new();
    // One default rule can disagree with many products' extensions; each
    // disagreement is said once.
    let mut disagreements = FxHashSet::default();
    // Each product's template is built once, for its rule, its shown path
    // and its checks.
    let templates = index.path_templates();
    for (id, product) in pipeline.products.iter().enumerate() {
        let template = templates[id].as_deref();
        let rule = PathRule::for_product(&index, &product.name, template);
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
                if problem
                    .said_once
                    .is_none_or(|said| disagreements.insert(said))
                {
                    errors.push(error(problem.message).at(line.clone()));
                }
            }
            // `parse` checked the rule as written; a dropped group can leave
            // two open shapes touching in this product's path.
            if let Some(Err(message)) = template.map(PathTemplate::check_open_shapes) {
                errors.push(
                    error(format!(
                        "in the path rule for `{}`, once the groups its dimensions lack are dropped: {message}",
                        product.name
                    ))
                    .at(line.clone()),
                );
            }
            match validate_path_template(&index, product, template) {
                Err(e) => errors.push(e.at(line)),
                // A repeated product name is reported by the resolver as a duplicate.
                Ok(sample) => {
                    if let Some(other) = samples
                        .insert(sample.clone(), &product.name)
                        .filter(|other| *other != product.name)
                    {
                        // A `beside` output has no rule to change, so the way out
                        // is its sibling's rule, or the other product's.
                        let fix = match (index.beside(&product.name), index.beside(other)) {
                            (Some((sibling, _, _)), _) => beside_fix(&product.name, sibling, other),
                            (None, Some((sibling, _, _))) => {
                                beside_fix(other, sibling, &product.name)
                            }
                            (None, None) => {
                                "include `{@product}` or distinguish their path rules".to_owned()
                            }
                        };
                        errors.push(
                            error(format!(
                                "products `{other}` and `{}` bind to the same path `{sample}` for the same entities; {fix}",
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
            folder: index.is_folder(&product.name),
            rule,
            extension: index
                .added_extension(&product.name)
                .map(|(extension, source)| format!("`{extension}` from {source}")),
        });
    }
    for (path, product) in &samples {
        if let Some((directory, other)) = enclosing_path(&samples, path) {
            let (inner_made, outer_made) = (outputs.contains(product), outputs.contains(other));
            let what = match (index.is_folder(other), inner_made, outer_made) {
                // Nothing writes a source, so a source may sit in a source folder.
                (true, false, false) => continue,
                (false, _, _) => format!("a `{other}` file"),
                (true, _, true) => format!("a `{other}` folder, which its job writes whole"),
                (true, true, false) => {
                    format!("a `{other}` source folder, which no job may write inside")
                }
            };
            errors.push(
                error(format!(
                    "path rule for `{product}` puts files inside `{directory}`, the path of {what}, for the same entities; distinguish their path rules"
                ))
                .at(lines.path_rule(&index, product)),
            );
        }
    }
    (PathCoverage { entries }, errors)
}

/// Ends the message about a product's own rule ending in another extension,
/// since a `.` that belongs to a name, as in `acq-1.5T`, starts one.
/// Keep in step with `PathTemplate::extension`, which reads it so.
const EXTENSION_START: &str =
    "; SPIT reads the extension from the first `.` after the last placeholder, so keep `.` out of the name before it";

/// What to change when `beside`, written beside `sibling`, has the path of
/// `other`: it has no rule of its own, so change `other`'s or `sibling`'s.
fn beside_fix(beside: &str, sibling: &str, other: &str) -> String {
    format!("`{beside}` follows `{sibling}`'s path, so change the path rule of `{other}` or of `{sibling}`")
}

/// A path rule that ends with an extension other than the one its file must
/// have.
struct Disagreement<'p> {
    message: String,
    /// What a default rule's message is about, since one default can
    /// disagree with many products and is said once: the stage whose default
    /// it is, the extension it ends with, the one it should, and where that
    /// is declared. `None` for a product's own rule, which is said for each.
    said_once: Option<(Option<&'p str>, &'p str, &'p str, ExtensionSource)>,
}

/// Why `product`'s path rule ends with an extension other than the one its
/// file must have, if it does. A rule that ends with none is given it.
fn extension_disagreement<'p>(
    index: &PipelineIndex<'p>,
    product: &str,
) -> Option<Disagreement<'p>> {
    let (expected, source) = index.expected_extension(product)?;
    let rule = index.path_rule_for(product)?;
    // A `.` earlier in the file name is not part of its extension.
    if rule.ends_with(expected) {
        return None;
    }
    let written = rule.extension()?;
    // A tool writes an output's file; a source's is declared.
    let verb = match source {
        ExtensionSource::Source(_) => "declares",
        _ => "writes",
    };
    let origin = index.path_origin(product).map(|(origin, _)| origin);
    if origin == Some(PathOrigin::Explicit) {
        return Some(Disagreement {
            message: format!(
                "path `{product}` ends in `{written}`, but {source} {verb} `{expected}`; drop the extension or use `{expected}`{EXTENSION_START}"
            ),
            said_once: None,
        });
    }
    let (stage, default) = match origin {
        Some(PathOrigin::Stage(stage)) => (Some(stage), format!("stage `{stage}`'s default path")),
        _ => (None, "the default path".to_owned()),
    };
    let said_once = Some((stage, written, expected, source.clone()));
    let message = match source {
        ExtensionSource::Operation(_) => format!(
            "{default} ends in `{written}`, but {source} writes `{expected}`; write {default} without an extension, and give the outputs that use it `ext: {written}`"
        ),
        ExtensionSource::Source(_) => format!(
            "{default} ends in `{written}`, but {source} declares `{expected}`; write {default} without an extension, so each source's declared extension completes it"
        ),
        ExtensionSource::Stage(_) | ExtensionSource::Default => format!(
            "{default} ends in `{written}`, but {source} sets `{expected}`; write the extension once, with `ext:`"
        ),
    };
    Some(Disagreement { message, said_once })
}

/// Bind a product's path rule to placeholder entities, rejecting rules that
/// cannot tell the product's artifacts apart. Returns the sample path.
fn validate_path_template(
    index: &PipelineIndex<'_>,
    product: &ProductDef,
    template: Option<&PathTemplate>,
) -> Result<String, PathError> {
    let template = template
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
            !placeholders.iter().any(
                |placeholder| matches!(placeholder, PathPlaceholder::Dimension(name, _) if name == *dimension),
            )
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
    bind_path(
        index,
        template,
        &product.dimensions,
        artifact.view(),
        || format!("path rule for `{}`", product.name),
    )
}

/// A shape narrows the files a source's rule finds, so it has no place in a
/// rule that also writes paths: an output's own rule, or a `path:` default
/// for a pipeline or a stage, which outputs take. Each is an error, since a
/// check that never ran would pass.
fn shape_errors(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
    outputs: &BTreeSet<&str>,
    errors: &mut Vec<PathError>,
) {
    let mut report = |template: &PathTemplate, line: Option<Place>, owner: String| {
        for (dimension, shape) in template.shaped() {
            let placeholder = format!("{{{dimension}:{shape}}}");
            errors.push(
                error(format!(
                    "`{placeholder}` has a shape, but shapes narrow a source's path rule only; {owner}"
                ))
                .at(line.clone())
                .focus(placeholder),
            );
        }
    };
    if let Some(template) = &pipeline.path_template {
        let owner = "`path:` is the default for outputs too; write the shape in a `path` rule for each source, as `path <source>: ...`".to_owned();
        report(template, lines.default_path.clone(), owner);
    }
    for stage in &pipeline.stages {
        if let Some(template) = &stage.path_template {
            let owner = format!(
                "stage `{}`'s `path:` is the default for its products; write the shape in a `path` rule for each source, as `path <source>: ...`",
                stage.name
            );
            report(template, lines.stage_paths.get(&stage.name).cloned(), owner);
        }
    }
    for (product, template) in &pipeline.product_paths {
        if !skip.contains(product) && outputs.contains(product.as_str()) {
            let owner = format!("`{product}` is made by a step");
            report(template, lines.paths.get(product).cloned(), owner);
        }
    }
}
