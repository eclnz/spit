use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustc_hash::FxHashSet;

use crate::model::{
    ArtifactInstance, BesideFile, EntityBinding, ExtensionSource, PathOrigin, Pipeline,
    PipelineIndex, ProductDef,
};
use crate::parser::SourceMap;
use crate::span::Place;

use super::components::enclosing_path;
use super::product::{bind_path, shown_paths};
use crate::template::{error, PathError, PathPart, PathPlaceholder, PathTemplate};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathRule {
    Explicit(String),
    Recipe(String),
    RecipeDefault(String),
    Stage { stage: String, template: String },
    Default(String),
    Beside { sibling: String, template: String },
    Inventory,
    Missing,
}

impl PathRule {
    fn for_product(
        index: &PipelineIndex<'_>,
        product: &str,
        template: Option<&PathTemplate>,
    ) -> Self {
        let beside = index.beside(product);
        let Some(template) = template else {
            return match beside {
                Some(beside) if beside.input.is_some() => Self::Beside {
                    sibling: beside.sibling.to_owned(),
                    template: format!("{}{}", if beside.append { "+" } else { "" }, beside.suffix),
                },
                _ => Self::Missing,
            };
        };
        let rule_product = beside.map_or(product, |beside| beside.sibling);
        let template = if index
            .path_rule_for(rule_product)
            .is_some_and(PathTemplate::varies)
        {
            shown_paths(index, product, template).join(" or ")
        } else {
            template
                .rules()
                .map(PathTemplate::as_str)
                .collect::<Vec<_>>()
                .join(" or ")
        };
        match (beside, index.path_origin(product)) {
            (Some(BesideFile { sibling, .. }), _) => Self::Beside {
                sibling: sibling.to_owned(),
                template,
            },
            (None, Some((PathOrigin::Explicit, _))) => Self::Explicit(template),
            (None, Some((PathOrigin::Stage(stage), _))) => Self::Stage {
                stage: stage.to_owned(),
                template,
            },
            (None, Some((PathOrigin::Default, _)) | None) => Self::Default(template),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverageEntry {
    pub product: String,
    pub source: bool,
    pub folder: bool,
    pub rule: PathRule,
    pub extension: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverage {
    pub entries: Vec<PathCoverageEntry>,
}

impl PathCoverage {
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

pub fn inspect_paths(pipeline: &Pipeline) -> Result<PathCoverage, PathError> {
    let (coverage, errors) = collect_paths(pipeline, &SourceMap::default(), &BTreeSet::new());
    match errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(coverage),
    }
}

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
    let mut misspelled: Vec<&PathTemplate> = Vec::new();
    for (template, line) in rules {
        let unknown: BTreeSet<_> = template
            .group_dimensions()
            .filter(|dimension| !had.contains(dimension))
            .collect();
        if !unknown.is_empty() {
            misspelled.push(template);
        }
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
    let mut disagreements = FxHashSet::default();
    let templates = index.path_templates();
    for (id, product) in pipeline.products.iter().enumerate() {
        let template = templates[id].as_deref();
        let rule = PathRule::for_product(&index, &product.name, template);
        if rule == PathRule::Missing
            && outputs.contains(product.name.as_str())
            && index.beside(&product.name).is_none()
            && !skip.contains(&product.name)
        {
            errors.push(
                error(format!(
                    "output `{0}` has no path rule; write `path {0}: ...`, or a `default path:` for the pipeline or its stage",
                    product.name
                ))
                .at(lines.products.get(&product.name).cloned()),
            );
        }
        if let Some(problem) = beside_input_problem(&index, &product.name) {
            if !skip.contains(&product.name) {
                errors.push(error(problem).at(lines.products.get(&product.name).cloned()));
            }
        }
        if template.is_some() && !skip.contains(&product.name) {
            let line = lines.path_rule(&index, &product.name);
            if let Some(BesideFile { sibling, .. }) = index.beside(&product.name) {
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
            let source =
                !outputs.contains(product.name.as_str()) && index.beside(&product.name).is_none();
            if let Some(Err(message)) = template.map(|template| template.check_touching(source)) {
                let written = matches!(templates[id], Some(Cow::Borrowed(_)));
                let grouped = index
                    .path_rule_for(&product.name)
                    .is_some_and(PathTemplate::varies);
                let message = match (written, grouped) {
                    (true, _) => message,
                    (false, true) => format!(
                        "in the path rule for `{}`, once the groups its dimensions lack are dropped: {message}",
                        product.name
                    ),
                    (false, false) => format!(
                        "in the path rule for `{}`, with `{{@entities}}` spelled out: {message}",
                        product.name
                    ),
                };
                errors.push(error(message).at(line.clone()));
            }
            let misspells = index
                .path_rule_for(&product.name)
                .is_some_and(|rule| misspelled.iter().any(|bad| std::ptr::eq(*bad, rule)));
            match (!misspells).then(|| validate_path_template(&index, product, template)) {
                None => {}
                Some(Err(e)) => errors.push(e.at(line)),
                Some(Ok(None)) => {}
                Some(Ok(Some(sample))) => {
                    if let Some(other) = samples
                        .insert(sample.clone(), &product.name)
                        .filter(|other| *other != product.name)
                    {
                        let fix = match (index.beside(&product.name), index.beside(other)) {
                            (Some(BesideFile { sibling, .. }), _) => {
                                beside_fix(&product.name, sibling, other)
                            }
                            (None, Some(BesideFile { sibling, .. })) => {
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
            if index.is_view(other)
                && index.producer(other).is_some_and(|(invocation, _)| {
                    invocation
                        .inputs
                        .iter()
                        .any(|input| input.product_name() == *product)
                })
            {
                continue;
            }
            let (inner_made, outer_made) = (outputs.contains(product), outputs.contains(other));
            let what = match (index.is_folder(other), inner_made, outer_made) {
                (true, false, false) => continue,
                (false, _, _) => format!("a `{other}` file"),
                (true, _, true) if index.is_view(other) => {
                    format!("directory view `{other}`, which does not list it")
                }
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

const EXTENSION_START: &str =
    "; SPIT reads the extension from the first `.` after the last placeholder, so keep `.` out of the name before it";

fn beside_input_problem(index: &PipelineIndex<'_>, product: &str) -> Option<String> {
    let beside = index.beside(product)?;
    beside.input?;
    let sibling = beside.sibling;
    if index.is_folder(sibling) {
        return Some(format!(
            "`{product}` is written beside `{sibling}`, which is a folder, and only a file has files beside it; give `{product}` a path of its own"
        ));
    }
    let declare = if index.is_source(sibling) {
        format!("declare one, as in `source {sibling} : Type .ext`")
    } else {
        "give the output that writes it one".to_owned()
    };
    (!beside.append && beside.replaces.is_none()).then(|| {
        format!(
            "`{product}` is written beside `{sibling}`, which declares no extension for `{}` to replace; {declare}, or append to its whole name with `+{}`",
            beside.suffix, beside.suffix
        )
    })
}

fn beside_fix(beside: &str, sibling: &str, other: &str) -> String {
    format!("`{beside}` follows `{sibling}`'s path, so change the path rule of `{other}` or of `{sibling}`")
}

struct Disagreement<'p> {
    message: String,
    said_once: Option<(Option<&'p str>, &'p str, &'p str, ExtensionSource)>,
}

fn extension_disagreement<'p>(
    index: &PipelineIndex<'p>,
    product: &str,
) -> Option<Disagreement<'p>> {
    let (expected, source) = index.expected_extension(product)?;
    let rule = index.path_rule_for(product)?;
    if rule.ends_with(expected) {
        let ending = index.naming_ending(product)?;
        if rule.ends_with(ending) {
            return None;
        }
        return Some(Disagreement {
            message: format!(
                "path `{product}` ends in `{expected}`, but {source} writes it with a naming suffix; drop the extension, and SPIT adds `{ending}`, or end the rule with `{ending}`"
            ),
            said_once: None,
        });
    }
    let written = rule.extension()?;
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
            "{default} ends in `{written}`, but {source} sets `{expected}`; write the extension once, with `default ext:`"
        ),
    };
    Some(Disagreement { message, said_once })
}

fn validate_path_template(
    index: &PipelineIndex<'_>,
    product: &ProductDef,
    template: Option<&PathTemplate>,
) -> Result<Option<String>, PathError> {
    let template = template
        .ok_or_else(|| error(format!("no path template for product `{}`", product.name)))?;
    let several = template.is_several();
    for rule in template.rules() {
        let extra = rule
            .bound_dimensions()
            .into_iter()
            .find(|dimension| !product.dimensions.iter().any(|own| own == dimension));
        if let Some(extra) = extra.filter(|_| !template.writes_paths()) {
            return Err(error(format!(
                "path rule `{}` binds `{extra}`, which `{}` does not have; declare it among the source's dimensions or remove it from the rule",
                rule.as_str(),
                product.name
            )));
        }
        if rule
            .parts()
            .contains(&PathPart::Placeholder(PathPlaceholder::Entities))
        {
            continue;
        }
        let bound = rule.bound_dimensions();
        if let Some(dimension) = product
            .dimensions
            .iter()
            .find(|dimension| !bound.contains(dimension.as_str()))
        {
            let message = if several {
                format!(
                    "path rule `{}` for `{}` omits dimension `{dimension}`; each rule of a source binds every dimension it has, from a name or as a fixed value such as `{{{dimension}=none}}`",
                    rule.as_str(),
                    product.name
                )
            } else {
                format!(
                    "path template for `{}` omits dimension `{dimension}`; artifacts differing only in `{dimension}` would share a path",
                    product.name
                )
            };
            return Err(error(message));
        }
    }
    if !template.writes_paths() {
        return Ok(None);
    }
    let entities: EntityBinding = product
        .dimensions
        .iter()
        .map(|dimension| (dimension.clone(), dimension.clone()))
        .collect();
    let artifact = ArtifactInstance::new(&product.name, product.artifact_type.clone(), entities);
    bind_path(index, template, artifact.view(), || {
        format!("path rule for `{}`", product.name)
    })
    .map(Some)
}

fn shape_errors(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
    outputs: &BTreeSet<&str>,
    errors: &mut Vec<PathError>,
) {
    let mut report = |template: &PathTemplate,
                      line: Option<Place>,
                      owner: &dyn Fn(&str) -> String| {
        for (dimension, shape) in template.shaped() {
            let placeholder = format!("{{{dimension}:{shape}}}");
            errors.push(
                error(format!(
                    "`{placeholder}` has a shape, but shapes narrow a source's path rule only; {}",
                    owner("the shape")
                ))
                .at(line.clone())
                .focus(placeholder),
            );
        }
        match template.reads_only() {
            Some(PathPart::Wildcard) => errors.push(
                error(format!(
                    "`*` matches any text, but only a source's path rule reads names; {}",
                    owner("the `*`")
                ))
                .at(line.clone())
                .focus("*"),
            ),
            Some(PathPart::Fixed { dimension, value }) => {
                let fixed = format!("{{{dimension}={value}}}");
                errors.push(
                    error(format!(
                        "`{fixed}` fixes a value, but only a source's path rule fixes one; {}",
                        owner("the fixed value")
                    ))
                    .at(line.clone())
                    .focus(fixed),
                );
            }
            _ => {}
        }
        if template.is_several() {
            errors.push(
                error(format!(
                    "this rule has several lines, but only a source reads its files with several rules; {}",
                    owner("the rules")
                ))
                .at(line.clone()),
            );
        }
    };
    if let Some(template) = &pipeline.path_template {
        report(template, lines.default_path.clone(), &|what| {
            format!("`default path:` is the rule for outputs too; write {what} in a `path` rule for each source, as `path <source>: ...`")
        });
    }
    for stage in &pipeline.stages {
        if let Some(template) = &stage.path_template {
            report(
                template,
                lines.stage_paths.get(&stage.name).cloned(),
                &|what| {
                    format!(
                    "stage `{}`'s `default path:` is the rule for its products; write {what} in a `path` rule for each source, as `path <source>: ...`",
                    stage.name
                )
                },
            );
        }
    }
    for (product, template) in &pipeline.product_paths {
        if !skip.contains(product) && outputs.contains(product.as_str()) {
            report(template, lines.paths.get(product).cloned(), &|_| {
                format!("`{product}` is made by a step")
            });
        }
    }
}
