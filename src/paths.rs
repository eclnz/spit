//! Path rules: product path coverage, artifact path binding, and source-file checks.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::fs;
use std::path::Path;

use crate::model::{
    ArtifactInstance, ArtifactKey, EntityBinding, Pipeline, ProductDef, ResolvedDag,
    SourceInventory, SourceRecord,
};
use crate::parser::SourceMap;
use crate::span::Located;
use crate::template::{parse_template, Part};

/// An error in a path rule, or about the paths it gives artifacts.
pub type PathError = Located<String>;

/// What a `{name}` in a path template stands for.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum PathPlaceholder {
    /// `{product}`: the product's name.
    Product,
    /// `{entities}`: every dimension as `dim=value`, in declared order.
    Entities,
    /// `{stage}`: the stage whose block holds the step.
    Stage,
    /// Any other name: a dimension the product declares.
    Dimension(String),
}

impl PathPlaceholder {
    /// The built-in placeholder `name` always means, if any; no product may
    /// declare a dimension with such a name.
    pub(crate) fn reserved(name: &str) -> Option<Self> {
        match name {
            "product" => Some(Self::Product),
            "entities" => Some(Self::Entities),
            "stage" => Some(Self::Stage),
            _ => None,
        }
    }

    fn parse(name: String) -> Self {
        Self::reserved(&name).unwrap_or(Self::Dimension(name))
    }

    /// The name between the braces.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Product => "product",
            Self::Entities => "entities",
            Self::Stage => "stage",
            Self::Dimension(name) => name,
        }
    }
}

/// Reads as the placeholder is written, such as `{stage}`.
impl fmt::Display for PathPlaceholder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}}}", self.name())
    }
}

/// A path template's literal text and placeholders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PathPart {
    Literal(String),
    Placeholder(PathPlaceholder),
}

/// A path rule's template, parsed once when the rule is read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathTemplate {
    text: String,
    parts: Vec<PathPart>,
}

impl PathTemplate {
    /// Parse a template such as `derivatives/{stage}/{product}/{entities}.mif`.
    pub fn parse(text: impl Into<String>) -> Result<Self, PathError> {
        let text = text.into();
        let parts = parse_template(&text)
            .map_err(error)?
            .into_iter()
            .map(|part| match part {
                Part::Literal(value) => PathPart::Literal(value),
                Part::Placeholder(name) => PathPart::Placeholder(PathPlaceholder::parse(name)),
            })
            .collect();
        Ok(Self { text, parts })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn parts(&self) -> &[PathPart] {
        &self.parts
    }

    /// This template with `{product}` written out as `name`, so that an
    /// imported product keeps the path its own file gives it.
    #[must_use]
    pub(crate) fn with_product(&self, name: &str) -> Self {
        let parts: Vec<_> = self
            .parts
            .iter()
            .map(|part| match part {
                PathPart::Placeholder(PathPlaceholder::Product) => {
                    PathPart::Literal(name.to_owned())
                }
                part => part.clone(),
            })
            .collect();
        let text = parts
            .iter()
            .map(|part| match part {
                PathPart::Literal(value) => value.replace('{', "{{").replace('}', "}}"),
                PathPart::Placeholder(placeholder) => placeholder.to_string(),
            })
            .collect();
        Self { text, parts }
    }
}

impl fmt::Display for PathTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq<str> for PathTemplate {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<&str> for PathTemplate {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

pub(crate) fn error(message: impl Into<String>) -> PathError {
    PathError::new(message)
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
            PathRule::Explicit(template.to_string())
        } else if let Some((stage, template)) = pipeline.stage_path_rule(&product.name) {
            PathRule::Stage {
                stage: stage.to_owned(),
                template: template.to_string(),
            }
        } else if let Some(template) = &pipeline.path_template {
            PathRule::Default(template.to_string())
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
) -> Result<VerifiedFiles, PathError> {
    if !root.is_dir() {
        return Err(error(format!(
            "source root is not a directory: `{}`",
            root.display()
        )));
    }
    inspect_paths(pipeline)?.validate(false)?;
    let paths = bound_paths(pipeline, dag)?;
    let outputs = output_keys(dag);
    let mut verified = VerifiedFiles::default();
    for (artifact, relative) in paths {
        if outputs.contains(&artifact) {
            continue;
        }
        // With `--stage`, what other stages make must already exist.
        let made_by = pipeline
            .invocations
            .iter()
            .find(|invocation| invocation.outputs.contains(&artifact.0));
        let full_path = root.join(&relative);
        if !full_path.is_file() {
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
        if made_by.is_some() {
            verified.made_elsewhere += 1;
        } else {
            verified.sources += 1;
        }
    }
    Ok(verified)
}

/// The files [`validate_source_files`] found under the root.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VerifiedFiles {
    /// Files of source products.
    pub sources: usize,
    /// Outputs of steps whose jobs the DAG leaves out, such as another
    /// stage's when one stage is selected.
    pub made_elsewhere: usize,
}

impl fmt::Display for VerifiedFiles {
    /// Reads as `3 source files verified.`, naming only counts above zero.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.sources > 0 || self.made_elsewhere == 0 {
            parts.push(format!("{} source files", self.sources));
        }
        if self.made_elsewhere > 0 {
            parts.push(format!(
                "{} files made outside the stage",
                self.made_elsewhere
            ));
        }
        write!(f, "{} verified.", parts.join(" and "))
    }
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
    for part in template.parts() {
        match part {
            PathPart::Literal(value) => relative.push_str(value),
            PathPart::Placeholder(PathPlaceholder::Product) => {
                // `alias::name` would put colons in file names.
                relative.push_str(&artifact.product.replace("::", "."));
            }
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                let stage = pipeline.stage_of(&artifact.product).ok_or_else(|| {
                    error(format!(
                        "path template for `{}` uses `{}`, but `{}` is not made in a stage",
                        artifact.product,
                        PathPlaceholder::Stage,
                        artifact.product
                    ))
                    .focus(PathPlaceholder::Stage.to_string())
                })?;
                // Each nested stage is a directory.
                let components: Vec<_> = stage.split('/').map(encode_component).collect();
                relative.push_str(&components.join("/"));
            }
            PathPart::Placeholder(PathPlaceholder::Entities) => {
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
            PathPart::Placeholder(placeholder @ PathPlaceholder::Dimension(dimension)) => {
                let value = artifact.entities.0.get(dimension).ok_or_else(|| {
                    error(format!(
                        "path template for `{}` uses absent dimension `{dimension}`",
                        artifact.product
                    ))
                    .focus(placeholder.to_string())
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

fn path_pattern(template: &PathTemplate, product: &ProductDef) -> Result<Vec<Piece>, PathError> {
    let mut pieces = Vec::new();
    for part in template.parts() {
        match part {
            PathPart::Literal(value) => pieces.push(Piece::Literal(value.clone())),
            PathPart::Placeholder(PathPlaceholder::Product) => {
                pieces.push(Piece::Literal(product.name.replace("::", ".")));
            }
            PathPart::Placeholder(PathPlaceholder::Entities) => {
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
            // A source is made in no stage, so its rule never binds
            // `{stage}`; `inspect_paths` rejects such a rule first.
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                return Err(error(format!(
                    "path rule for source `{}` uses `{}`, but a source is not made in a stage",
                    product.name,
                    PathPlaceholder::Stage
                )))
            }
            PathPart::Placeholder(PathPlaceholder::Dimension(dimension)) => {
                pieces.push(Piece::Value(dimension.clone()));
            }
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
