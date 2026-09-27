//! Path rules: product path coverage, artifact path binding, and source-file checks.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::ops::Range;
use std::path::Path;

use crate::model::{
    ArtifactInstance, ArtifactKey, EntityBinding, Pipeline, ProductDef, ResolvedDag,
};
use crate::parser::SourceMap;
use crate::span::Place;
use crate::template::{parse_template, Part};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathError {
    /// The pipeline line of the path rule at fault, when known.
    pub line: Option<usize>,
    /// The byte range in that line, when known.
    pub columns: Option<Range<usize>>,
    pub message: String,
    /// Text within the path template that the error is about, such as one
    /// `{placeholder}`.
    pub(crate) focus: Option<String>,
}

impl PathError {
    /// Attach a place unless a more specific one is already recorded.
    fn at(mut self, place: Option<Place>) -> Self {
        if self.line.is_none() {
            if let Some(place) = place {
                self.line = Some(place.line);
                self.columns = Some(place.columns);
            }
        }
        self
    }

    fn focus(mut self, text: impl Into<String>) -> Self {
        self.focus = Some(text.into());
        self
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for PathError {}

pub(crate) fn error(message: impl Into<String>) -> PathError {
    PathError {
        line: None,
        columns: None,
        message: message.into(),
        focus: None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathRule {
    Explicit(String),
    Default(String),
    Missing,
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
                .filter(|entry| matches!(entry.rule, PathRule::Default(_)))
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
                PathRule::Default(template) => {
                    writeln!(f, "  {} ({role}): default {template}", entry.product)?;
                }
                PathRule::Missing => {
                    writeln!(f, "  {} ({role}): MISSING", entry.product)?;
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
        .map(|invocation| invocation.output_product.as_str())
        .collect();
    let mut entries = Vec::new();
    let mut samples: BTreeMap<String, &str> = BTreeMap::new();
    for product in &pipeline.products {
        let rule = if let Some(template) = pipeline.product_paths.get(&product.name) {
            PathRule::Explicit(template.clone())
        } else if let Some(template) = &pipeline.path_template {
            PathRule::Default(template.clone())
        } else {
            PathRule::Missing
        };
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
    (PathCoverage { entries }, errors)
}

/// Bind a product's path rule to placeholder entities, rejecting rules that
/// cannot tell the product's artifacts apart. Returns the sample path.
fn validate_path_template(pipeline: &Pipeline, product: &ProductDef) -> Result<String, PathError> {
    let template = pipeline
        .product_paths
        .get(&product.name)
        .or(pipeline.path_template.as_ref())
        .ok_or_else(|| error(format!("no path template for product `{}`", product.name)))?;
    let placeholders: BTreeSet<_> = parse_template(template)
        .map_err(error)?
        .into_iter()
        .filter_map(|part| match part {
            Part::Placeholder(name) => Some(name),
            Part::Literal(_) => None,
        })
        .collect();
    if !placeholders.contains("entities") {
        if let Some(dimension) = product
            .dimensions
            .iter()
            .find(|dimension| !placeholders.contains(*dimension))
        {
            return Err(error(format!(
                "path template for `{}` omits dimension `{dimension}`; artifacts differing only in `{dimension}` would share a path",
                product.name
            )));
        }
    }
    // Each dimension gets a distinct sample value so that templates naming
    // different dimensions are not mistaken for colliding ones.
    let entities = EntityBinding(
        product
            .dimensions
            .iter()
            .map(|dimension| (dimension.clone(), dimension.clone()))
            .collect(),
    );
    let artifact = ArtifactInstance::new(&product.name, product.artifact_type.clone(), entities);
    bind_path(pipeline, &product.dimensions, &artifact)
}

/// Check placeholder brackets in a path template.
pub(crate) fn check_path_template_syntax(template: &str) -> Result<(), PathError> {
    parse_template(template).map(|_| ()).map_err(error)
}

/// Validate concrete artifact path bindings without requiring commands.
pub fn validate_concrete_paths(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<(), PathError> {
    bound_paths(pipeline, dag)?;
    Ok(())
}

/// Check the files needed to start the resolved DAG under a dataset root.
/// Derived outputs are deliberately excluded because the pipeline creates them.
pub fn validate_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<usize, PathError> {
    if !root.is_dir() {
        return Err(error(format!(
            "source root is not a directory: `{}`",
            root.display()
        )));
    }
    inspect_paths(pipeline)?.validate(false)?;
    let paths = bound_paths(pipeline, dag)?;
    let outputs: BTreeSet<_> = dag.jobs.iter().map(|job| job.output.key()).collect();
    let mut checked = 0;
    for (artifact, relative) in paths {
        if outputs.contains(&artifact) {
            continue;
        }
        let full_path = root.join(&relative);
        if !full_path.is_file() {
            return Err(error(format!(
                "missing source file for `{}[{}]`: `{}`",
                artifact.0,
                artifact.1,
                full_path.display()
            )));
        }
        checked += 1;
    }
    Ok(checked)
}

pub(crate) fn bound_paths(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
) -> Result<BTreeMap<ArtifactKey, String>, PathError> {
    let mut paths = BTreeMap::new();
    let mut owners = BTreeMap::new();
    for artifact in dag
        .jobs
        .iter()
        .flat_map(|job| job.inputs.iter().chain(std::iter::once(&job.output)))
    {
        let identity = artifact.key();
        if paths.contains_key(&identity) {
            continue;
        }
        let dimensions = dag
            .product_dimensions
            .get(&artifact.product)
            .ok_or_else(|| error(format!("unknown product `{}`", artifact.product)))?;
        let relative = bind_path(pipeline, dimensions, artifact)?;
        if let Some(previous) = owners.insert(relative.clone(), identity.clone()) {
            return Err(error(format!(
                "artifacts `{}[{}]` and `{}[{}]` bind to the same path `{relative}`",
                previous.0, previous.1, identity.0, identity.1
            )));
        }
        paths.insert(identity, relative);
    }
    Ok(paths)
}

/// Bind `artifact` to its relative path. `dimensions` gives the product's
/// declared dimension order, which `{entities}` follows.
fn bind_path(
    pipeline: &Pipeline,
    dimensions: &[String],
    artifact: &ArtifactInstance,
) -> Result<String, PathError> {
    let template = pipeline
        .product_paths
        .get(&artifact.product)
        .or(pipeline.path_template.as_ref())
        .ok_or_else(|| {
            error(format!(
                "no path template for product `{}`",
                artifact.product
            ))
        })?;
    let mut relative = String::new();
    for part in parse_template(template).map_err(error)? {
        match part {
            Part::Literal(value) => relative.push_str(&value),
            Part::Placeholder(name) if name == "product" => {
                // `alias::name` would put colons in file names.
                relative.push_str(&artifact.product.replace("::", "."));
            }
            Part::Placeholder(name) if name == "entities" => {
                let bindings = dimensions
                    .iter()
                    .map(|dimension| {
                        let value = artifact.entities.0.get(dimension).ok_or_else(|| {
                            error(format!(
                                "artifact `{artifact}` lacks dimension `{dimension}`"
                            ))
                        })?;
                        Ok(format!(
                            "{}={}",
                            encode_component(dimension),
                            encode_component(value)
                        ))
                    })
                    .collect::<Result<Vec<_>, PathError>>()?;
                if bindings.is_empty() {
                    relative.push_str("global");
                } else {
                    relative.push_str(&bindings.join("__"));
                }
            }
            Part::Placeholder(dimension) => {
                let value = artifact.entities.0.get(&dimension).ok_or_else(|| {
                    error(format!(
                        "path template for `{}` uses absent dimension `{dimension}`",
                        artifact.product
                    ))
                    .focus(format!("{{{dimension}}}"))
                })?;
                relative.push_str(&encode_component(value));
            }
        }
    }
    if relative
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(error(format!(
            "path for `{artifact}` must be a relative path without `.` or `..`: `{relative}`"
        )));
    }
    Ok(relative)
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}
