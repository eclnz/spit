//! Path templates and the path one template gives one artifact. Every step
//! shares this: it knows the model, and nothing about resolving or discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustc_hash::FxHashMap;

use crate::model::{Artifact, DirectoryDiscovery, Pipeline};
use crate::span::Located;
use crate::template::{parse_template, Part};

crate::span::message_error!(
    /// What is wrong with a path rule, or with the paths it gives artifacts.
    PathProblem
);

/// An error in a path rule, or about the paths it gives artifacts.
pub type PathError = Located<PathProblem>;

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

/// Fail unless `root`, where a dataset's source files are, is a directory.
pub(crate) fn require_directory(root: &std::path::Path) -> Result<(), PathError> {
    if root.is_dir() {
        return Ok(());
    }
    Err(error(format!(
        "source root is not a directory: `{}`",
        root.display()
    )))
}

/// Bind `artifact` to its relative path. `dimensions` gives the product's
/// declared dimension order, which `{entities}` follows. `label` names the
/// path in errors: a product's rule, or an artifact.
pub(crate) fn bind_path(
    pipeline: &Pipeline,
    dimensions: &[String],
    artifact: Artifact<'_>,
    label: impl Fn() -> String,
) -> Result<String, PathError> {
    ProductPath::new(pipeline, artifact.product)?.bind(dimensions, artifact, label)
}

/// Binds many artifacts' paths, as [`bind_path`] does, finding each
/// product's template and stage once.
pub(crate) struct PathBinder<'p> {
    pipeline: &'p Pipeline,
    products: FxHashMap<String, ProductPath<'p>>,
    /// The same, by the product's number in a DAG's artifact table.
    numbered: Vec<Option<ProductPath<'p>>>,
}

impl<'p> PathBinder<'p> {
    pub(crate) fn new(pipeline: &'p Pipeline) -> Self {
        Self {
            pipeline,
            products: FxHashMap::default(),
            numbered: Vec::new(),
        }
    }

    /// As [`PathBinder::bind`], for an artifact whose product has `number`
    /// in its DAG's artifact table.
    pub(crate) fn bind_numbered(
        &mut self,
        number: u32,
        dimensions: &[String],
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let number = number as usize;
        if self.numbered.len() <= number {
            self.numbered.resize_with(number + 1, || None);
        }
        let product = match &mut self.numbered[number] {
            Some(product) => product,
            slot => slot.insert(ProductPath::new(self.pipeline, artifact.product)?),
        };
        product.bind(dimensions, artifact, label)
    }

    pub(crate) fn bind(
        &mut self,
        dimensions: &[String],
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let product = match self.products.get(artifact.product) {
            Some(product) => product,
            None => {
                let product = ProductPath::new(self.pipeline, artifact.product)?;
                self.products
                    .entry(artifact.product.to_owned())
                    .or_insert(product)
            }
        };
        product.bind(dimensions, artifact, label)
    }
}

/// What every path of one product shares: its template, its name as a
/// path gives it, and its stage's directories.
struct ProductPath<'p> {
    template: &'p PathTemplate,
    /// `alias::name` would put colons in file names.
    name: String,
    /// Each nested stage is a directory.
    stage: Option<String>,
}

impl<'p> ProductPath<'p> {
    fn new(pipeline: &'p Pipeline, product: &str) -> Result<Self, PathError> {
        let template = pipeline
            .path_template_for(product)
            .ok_or_else(|| error(format!("no path template for product `{product}`")))?;
        let stage = pipeline.stage_of(product).map(|stage| {
            let mut directories = String::new();
            for (index, component) in stage.split('/').enumerate() {
                if index > 0 {
                    directories.push('/');
                }
                push_encoded(&mut directories, component);
            }
            directories
        });
        Ok(Self {
            template,
            name: product.replace("::", "."),
            stage,
        })
    }

    fn bind(
        &self,
        dimensions: &[String],
        artifact: Artifact<'_>,
        label: impl Fn() -> String,
    ) -> Result<String, PathError> {
        let mut relative = String::with_capacity(self.template.text.len() + 32);
        for part in self.template.parts() {
            match part {
                PathPart::Literal(value) => relative.push_str(value),
                PathPart::Placeholder(PathPlaceholder::Product) => relative.push_str(&self.name),
                PathPart::Placeholder(PathPlaceholder::Stage) => {
                    let stage = self.stage.as_deref().ok_or_else(|| {
                        error(format!(
                            "path template for `{}` uses `{}`, but `{}` is not made in a stage",
                            artifact.product,
                            PathPlaceholder::Stage,
                            artifact.product
                        ))
                        .focus(PathPlaceholder::Stage.to_string())
                    })?;
                    relative.push_str(stage);
                }
                PathPart::Placeholder(PathPlaceholder::Entities) => {
                    push_entities(&mut relative, artifact, dimensions)?;
                }
                PathPart::Placeholder(placeholder @ PathPlaceholder::Dimension(dimension)) => {
                    let value = artifact.entities.get(dimension).ok_or_else(|| {
                        error(format!(
                            "path template for `{}` uses absent dimension `{dimension}`",
                            artifact.product
                        ))
                        .focus(placeholder.to_string())
                    })?;
                    push_encoded(&mut relative, value);
                }
            }
        }
        if let Some(reason) = unusable_path(&relative) {
            return Err(error(format!("{} {reason}: `{relative}`", label())));
        }
        Ok(relative)
    }
}

/// Add what `{entities}` binds to: each dimension as `dimension=value`, in
/// declared order and joined by `__`, or `global` for none.
fn push_entities(
    relative: &mut String,
    artifact: Artifact<'_>,
    dimensions: &[String],
) -> Result<(), PathError> {
    if dimensions.is_empty() {
        relative.push_str("global");
    }
    for (index, dimension) in dimensions.iter().enumerate() {
        let value = artifact.entities.get(dimension).ok_or_else(|| {
            error(format!(
                "artifact `{artifact}` lacks dimension `{dimension}`"
            ))
        })?;
        if index > 0 {
            relative.push_str("__");
        }
        push_encoded(relative, dimension);
        relative.push('=');
        push_encoded(relative, value);
    }
    Ok(())
}

/// Why `relative` cannot name a file under the root, if it cannot.
pub(crate) fn unusable_path(relative: &str) -> Option<&'static str> {
    if relative.starts_with('/') {
        return Some("must be relative to the dataset root, not start with `/`");
    }
    if relative.ends_with('/') {
        return Some("must name a file, not end with `/`");
    }
    let (mut empty, mut dots) = (false, false);
    for component in relative.as_bytes().split(|&byte| byte == b'/') {
        empty |= component.is_empty();
        dots |= component == b"." || component == b"..";
    }
    if empty {
        Some("must not contain an empty directory name, as in `//`")
    } else if dots {
        Some("must not contain `.` or `..` directories")
    } else {
        None
    }
}

/// `value` as one path component: ASCII letters, digits and `-` as they
/// are, every other byte as `%XX`.
///
/// Keep in step with `is_value_character` in `inputs/discover.rs`, which
/// holds that an encoded value has only these characters and `%`:
/// discovery binds a value without searching when the character after it
/// cannot be one of them, so a character added here must be added there.
pub(crate) fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    push_encoded(&mut encoded, value);
    encoded
}

/// Add `value` to `text` as [`encode_component`] encodes it. Runs of
/// bytes kept as they are are added whole.
fn push_encoded(text: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut plain = 0;
    for (index, byte) in value.bytes().enumerate() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            continue;
        }
        // A run kept as it is is ASCII, so it starts and ends between
        // characters; an empty one may not.
        if plain < index {
            text.push_str(&value[plain..index]);
        }
        text.push('%');
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0xF)]));
        plain = index + 1;
    }
    text.push_str(&value[plain..]);
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

#[cfg(test)]
mod tests {
    use super::{decode_component, encode_component, unusable_path};

    #[test]
    fn components_keep_letters_digits_and_dashes_and_encode_the_rest() {
        assert_eq!(encode_component("sub-01"), "sub-01");
        assert_eq!(encode_component("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(encode_component("éa"), "%C3%A9a");
        assert_eq!(encode_component("__"), "%5F%5F");
        assert_eq!(encode_component(""), "");
        assert_eq!(decode_component("a%20b%2F%C3%A9").as_deref(), Some("a b/é"));
    }

    #[test]
    fn unusable_paths_are_named_by_their_first_problem() {
        assert_eq!(unusable_path("a/b.txt"), None);
        assert!(unusable_path("/a").unwrap().contains("relative"));
        assert!(unusable_path("a/").unwrap().contains("end with"));
        assert!(unusable_path("a//../b").unwrap().contains("empty"));
        assert!(unusable_path("a/../b").unwrap().contains("`..`"));
        assert!(unusable_path("./b").unwrap().contains("`..`"));
    }
}
