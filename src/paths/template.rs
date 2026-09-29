//! Path templates and the path one template gives one artifact. Every step
//! shares this: it knows the model, and nothing about resolving or discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

use crate::model::{ArtifactInstance, DirectoryDiscovery, Pipeline};
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
    /// Where outputs go when a pipeline run from a recipe sets no `path:`.
    pub fn default_output() -> Self {
        Self::parse("out/{product}/{entities}").expect("built-in output path is valid")
    }

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

/// Bind `artifact` to its relative path. `dimensions` gives the product's
/// declared dimension order, which `{entities}` follows. `label` names the
/// path in errors: a product's rule, or an artifact.
pub(crate) fn bind_path(
    pipeline: &Pipeline,
    dimensions: &[String],
    artifact: &ArtifactInstance,
    label: impl Fn() -> String,
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
    if let Some(reason) = unusable_path(&relative) {
        return Err(error(format!("{} {reason}: `{relative}`", label())));
    }
    Ok(relative)
}

/// Why `relative` cannot name a file under the root, if it cannot.
pub(crate) fn unusable_path(relative: &str) -> Option<&'static str> {
    if relative.starts_with('/') {
        Some("must be relative to `SPIT_ROOT`, not start with `/`")
    } else if relative.ends_with('/') {
        Some("must name a file, not end with `/`")
    } else if relative.split('/').any(str::is_empty) {
        Some("must not contain an empty directory name, as in `//`")
    } else if relative
        .split('/')
        .any(|component| component == "." || component == "..")
    {
        Some("must not contain `.` or `..` directories")
    } else {
        None
    }
}

pub(crate) fn encode_component(value: &str) -> String {
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

pub(crate) fn decode_component(encoded: &str) -> Option<String> {
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

/// A directory of `path` that is itself a file in `paths`, with its owner.
pub(crate) fn enclosing_path<'a, T>(
    paths: &'a BTreeMap<String, T>,
    path: &str,
) -> Option<(&'a str, &'a T)> {
    path.match_indices('/').find_map(|(end, _)| {
        paths
            .get_key_value(&path[..end])
            .map(|(directory, owner)| (directory.as_str(), owner))
    })
}

/// Check that a directory rule names a valid context path.
pub(crate) fn validate_discovery_rule(rule: &DirectoryDiscovery) -> Result<(), PathError> {
    let dimensions: BTreeSet<_> = rule.dimensions.iter().collect();
    if rule.dimensions.is_empty() || dimensions.len() != rule.dimensions.len() {
        return Err(error(format!(
            "discovery `{}` needs distinct dimensions",
            rule.name
        )));
    }
    let mut used = BTreeSet::new();
    let mut sample = String::new();
    for part in rule.template.parts() {
        match part {
            PathPart::Literal(value) => sample.push_str(value),
            PathPart::Placeholder(PathPlaceholder::Dimension(name))
                if dimensions.contains(name) =>
            {
                used.insert(name);
                sample.push_str(name);
            }
            PathPart::Placeholder(placeholder) => {
                return Err(error(format!(
                    "discovery `{}` uses undeclared or reserved placeholder `{placeholder}`",
                    rule.name
                )));
            }
        }
    }
    if let Some(missing) = rule
        .dimensions
        .iter()
        .find(|dimension| !used.contains(dimension))
    {
        return Err(error(format!(
            "discovery `{}` pattern omits dimension `{missing}`",
            rule.name
        )));
    }
    if sample.is_empty() || sample.ends_with('/') {
        return Err(error(format!(
            "discovery `{}` must name a directory without a trailing `/`",
            rule.name
        )));
    }
    if let Some(reason) = unusable_path(&sample) {
        return Err(error(format!("discovery `{}` pattern {reason}", rule.name)));
    }
    Ok(())
}
