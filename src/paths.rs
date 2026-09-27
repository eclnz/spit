//! Path rules: product path coverage, artifact path binding, and source-file checks.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::fs;
use std::ops::Range;
use std::path::Path;

use crate::model::{
    ArtifactInstance, ArtifactKey, EntityBinding, Pipeline, ProductDef, ResolvedDag,
    SourceInventory, SourceRecord,
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
    /// The default `path:` rule written inside a stage.
    Stage {
        stage: String,
        template: String,
    },
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
        .flat_map(|invocation| invocation.outputs.iter().map(String::as_str))
        .collect();
    let mut entries = Vec::new();
    let mut samples: BTreeMap<String, &str> = BTreeMap::new();
    for product in &pipeline.products {
        let rule = if let Some(template) = pipeline.product_paths.get(&product.name) {
            PathRule::Explicit(template.clone())
        } else if let Some((stage, template)) = pipeline.stage_path_rule(&product.name) {
            PathRule::Stage {
                stage: stage.to_owned(),
                template: template.clone(),
            }
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
        .path_template_for(&product.name)
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
    let outputs = output_keys(dag);
    let mut checked = 0;
    for (artifact, relative) in paths {
        if outputs.contains(&artifact) {
            continue;
        }
        let full_path = root.join(&relative);
        if !full_path.is_file() {
            // With `--stage`, an earlier stage's outputs must already exist.
            let made_by = pipeline
                .invocations
                .iter()
                .find(|invocation| invocation.outputs.contains(&artifact.0));
            return Err(error(match made_by {
                Some(invocation) => format!(
                    "missing file for `{}[{}]`, which {} makes: `{}`",
                    artifact.0,
                    artifact.1,
                    invocation.stage.as_ref().map_or_else(
                        || "an earlier step".to_owned(),
                        |stage| format!("stage `{stage}`")
                    ),
                    full_path.display()
                ),
                None => format!(
                    "missing source file for `{}[{}]`: `{}`",
                    artifact.0,
                    artifact.1,
                    full_path.display()
                ),
            }));
        }
        checked += 1;
    }
    Ok(checked)
}

/// Every artifact the resolved jobs produce.
pub(crate) fn output_keys(dag: &ResolvedDag) -> BTreeSet<ArtifactKey> {
    dag.jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .map(ArtifactInstance::key)
        .collect()
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
        .flat_map(|job| job.input_artifacts().chain(&job.outputs))
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
        .path_template_for(&artifact.product)
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
            // A declared dimension named `stage` keeps its meaning.
            Part::Placeholder(name) if name == "stage" && !dimensions.contains(&name) => {
                let stage = pipeline.stage_of(&artifact.product).ok_or_else(|| {
                    error(format!(
                        "path template for `{}` uses `{{stage}}`, but `{}` is not made in a stage",
                        artifact.product, artifact.product
                    ))
                    .focus("{stage}")
                })?;
                // Each nested stage is a directory.
                let components: Vec<_> = stage.split('/').map(encode_component).collect();
                relative.push_str(&components.join("/"));
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

/// Find the source artifacts under `root`: each regular file whose path
/// matches the path rule of a source product becomes a record with the
/// entity values the rule's placeholders capture.
pub fn discover_sources(pipeline: &Pipeline, root: &Path) -> Result<SourceInventory, PathError> {
    if !root.is_dir() {
        return Err(error(format!(
            "source root is not a directory: `{}`",
            root.display()
        )));
    }
    inspect_paths(pipeline)?;
    let outputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| &invocation.outputs)
        .collect();
    let mut patterns = Vec::new();
    for product in &pipeline.products {
        if outputs.contains(&product.name) {
            continue;
        }
        let template = pipeline.path_template_for(&product.name).ok_or_else(|| {
            error(format!(
                "no path rule for source `{}`, so its files cannot be discovered",
                product.name
            ))
        })?;
        patterns.push((product, path_pattern(template, product)?));
    }
    let mut files = Vec::new();
    walk(root, "", &mut files)?;
    files.sort();
    let mut records = Vec::new();
    for file in &files {
        let mut owner: Option<&ProductDef> = None;
        for (product, pattern) in &patterns {
            let mut bound = BTreeMap::new();
            if !match_pattern(pattern, file, &mut bound) {
                continue;
            }
            if let Some(other) = owner {
                return Err(error(format!(
                    "file `{file}` matches the path rules of both `{}` and `{}`",
                    other.name, product.name
                )));
            }
            owner = Some(product);
            let mut entities = BTreeMap::new();
            for (dimension, encoded) in bound {
                let value = decode_component(encoded)
                    .filter(|value| {
                        !value.is_empty()
                            && !value.chars().any(|character| {
                                character.is_whitespace() || ",[]=#".contains(character)
                            })
                    })
                    .ok_or_else(|| {
                        error(format!(
                            "file `{file}` gives `{dimension}` the value `{encoded}`, which an inventory cannot hold"
                        ))
                    })?;
                entities.insert(dimension, value);
            }
            records.push(SourceRecord::new(&product.name, EntityBinding(entities)));
        }
    }
    let rank: BTreeMap<_, _> = pipeline
        .products
        .iter()
        .enumerate()
        .map(|(index, product)| (product.name.as_str(), (index, &product.dimensions)))
        .collect();
    records.sort_by(|left, right| {
        let (left_rank, dimensions) = rank[left.product.as_str()];
        left_rank
            .cmp(&rank[right.product.as_str()].0)
            .then_with(|| left.entities.cmp_in(&right.entities, dimensions))
    });
    Ok(SourceInventory {
        artifacts: records,
        contexts: Vec::new(),
    })
}

enum Piece {
    Literal(String),
    /// One path component value, as `bind_path` encodes it.
    Value(String),
}

fn path_pattern(template: &str, product: &ProductDef) -> Result<Vec<Piece>, PathError> {
    let mut pieces = Vec::new();
    for part in parse_template(template).map_err(error)? {
        match part {
            Part::Literal(value) => pieces.push(Piece::Literal(value)),
            Part::Placeholder(name) if name == "product" => {
                pieces.push(Piece::Literal(product.name.replace("::", ".")));
            }
            Part::Placeholder(name) if name == "entities" => {
                if product.dimensions.is_empty() {
                    pieces.push(Piece::Literal("global".to_owned()));
                }
                for (index, dimension) in product.dimensions.iter().enumerate() {
                    let separator = if index == 0 { "" } else { "__" };
                    pieces.push(Piece::Literal(format!(
                        "{separator}{}=",
                        encode_component(dimension)
                    )));
                    pieces.push(Piece::Value(dimension.clone()));
                }
            }
            Part::Placeholder(dimension) => pieces.push(Piece::Value(dimension)),
        }
    }
    Ok(pieces)
}

/// Match `text` against `pieces`, binding each dimension to its encoded
/// value; a dimension used twice must have the same value both times.
fn match_pattern<'a>(
    pieces: &[Piece],
    text: &'a str,
    bound: &mut BTreeMap<String, &'a str>,
) -> bool {
    match pieces.split_first() {
        None => text.is_empty(),
        Some((Piece::Literal(literal), rest)) => text
            .strip_prefix(literal.as_str())
            .is_some_and(|text| match_pattern(rest, text, bound)),
        Some((Piece::Value(dimension), rest)) => {
            if let Some(value) = bound.get(dimension).copied() {
                return text
                    .strip_prefix(value)
                    .is_some_and(|text| match_pattern(rest, text, bound));
            }
            let longest = text
                .find(|character: char| {
                    !(character.is_ascii_alphanumeric() || character == '-' || character == '%')
                })
                .unwrap_or(text.len());
            for end in 1..=longest {
                bound.insert(dimension.clone(), &text[..end]);
                if match_pattern(rest, &text[end..], bound) {
                    return true;
                }
            }
            bound.remove(dimension);
            false
        }
    }
}

fn decode_component(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = encoded.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// Collect every regular file under `directory`, as `/`-separated paths
/// relative to the root. Names that are not UTF-8 cannot match a rule.
fn walk(directory: &Path, prefix: &str, files: &mut Vec<String>) -> Result<(), PathError> {
    let entries = fs::read_dir(directory)
        .map_err(|reason| error(format!("cannot read `{}`: {reason}", directory.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|reason| error(format!("cannot read `{}`: {reason}", directory.display())))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let relative = format!("{prefix}{name}");
        let path = entry.path();
        // Directory links are not followed, so a link cycle cannot recurse.
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            walk(&path, &format!("{relative}/"), files)?;
        } else if path.is_file() {
            files.push(relative);
        }
    }
    Ok(())
}
