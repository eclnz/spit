//! Path templates and the path one template gives one artifact. Every step
//! shares this: it knows the model, and nothing about resolving or discovery.

use std::borrow::Cow;
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
    /// `{@product}`: the product's name.
    Product,
    /// `{@entities}`: every dimension as `dim=value`, in declared order.
    Entities,
    /// `{@stage}`: the stage whose block holds the step.
    Stage,
    /// `{@labels}`: every dimension as `dim-value`, joined by `_`, as BIDS
    /// names them. A product's template has it written out.
    Labels,
    /// Any other name: a dimension the product declares.
    Dimension(String),
}

impl PathPlaceholder {
    /// The built-in placeholder `{@name}` writes, if any.
    fn built_in(name: &str) -> Option<Self> {
        match name {
            "product" => Some(Self::Product),
            "entities" => Some(Self::Entities),
            "stage" => Some(Self::Stage),
            "labels" => Some(Self::Labels),
            _ => None,
        }
    }

    /// The placeholder `{name}` is: a built-in as `@product`, or else a
    /// dimension, which takes no `@`.
    fn parse(name: String) -> Result<Self, String> {
        match name.strip_prefix('@') {
            Some(built_in) => Self::built_in(built_in).ok_or_else(|| {
                format!(
                    "unknown built-in placeholder `{{{name}}}`; path templates have `{{@product}}`, `{{@entities}}`, `{{@labels}}` and `{{@stage}}`"
                )
            }),
            None => Ok(Self::Dimension(name)),
        }
    }

    /// What to write instead when a product has no dimension `name`, if
    /// `name` is a built-in written without its `@`.
    pub(crate) fn hint(name: &str) -> Option<String> {
        let meaning = match Self::built_in(name)? {
            Self::Product => "the product's name",
            Self::Entities => "every dimension as `dimension=value`",
            Self::Stage => "the stage that makes it",
            Self::Labels => "every dimension as `dimension-value`",
            Self::Dimension(_) => return None,
        };
        Some(format!("; write `{{@{name}}}` for {meaning}"))
    }

    /// The name between the braces, with a built-in's `@`.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Product => "@product",
            Self::Entities => "@entities",
            Self::Stage => "@stage",
            Self::Labels => "@labels",
            Self::Dimension(name) => name,
        }
    }
}

/// Reads as the placeholder is written, such as `{@stage}`.
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
    /// `[...]`: text and placeholders a product's path keeps only when each
    /// placeholder in it has a value for that product. A product's template
    /// has its groups resolved.
    Group(Vec<PathPart>),
}

/// What a product gives the placeholders of a path template: its
/// dimensions, in the pipeline's order, and whether a stage makes it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Holder<'a> {
    pub(crate) dimensions: &'a [String],
    pub(crate) in_stage: bool,
}

impl Holder<'_> {
    /// Whether `placeholder` has a value for the product.
    fn has(self, placeholder: &PathPlaceholder) -> bool {
        match placeholder {
            PathPlaceholder::Product | PathPlaceholder::Entities => true,
            PathPlaceholder::Stage => self.in_stage,
            PathPlaceholder::Labels => !self.dimensions.is_empty(),
            PathPlaceholder::Dimension(name) => self.dimensions.contains(name),
        }
    }

    /// Whether the product keeps `group`: each placeholder in it has a value.
    fn keeps(self, group: &[PathPart]) -> bool {
        group.iter().all(|part| match part {
            PathPart::Placeholder(placeholder) => self.has(placeholder),
            _ => true,
        })
    }
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
        Self::parse("out/{@product}/{@entities}").expect("built-in output path is valid")
    }

    /// Parse a template such as `derivatives/{@stage}/{@product}/{@entities}.mif`.
    pub fn parse(text: impl Into<String>) -> Result<Self, PathError> {
        let text = text.into();
        let parts = parse_parts(&text).map_err(error)?;
        Ok(Self { text, parts })
    }

    /// Whether the template has a `[...]` group or `{@labels}`, which each
    /// product resolves its own way.
    pub(crate) fn varies(&self) -> bool {
        self.parts.iter().any(|part| {
            matches!(
                part,
                PathPart::Group(_) | PathPart::Placeholder(PathPlaceholder::Labels)
            )
        })
    }

    /// This template as `holder`'s product has it: each group kept whole or
    /// dropped, and `{@labels}` written out as `sub-{sub}_ses-{ses}`. One
    /// with no group or `{@labels}` is the same for every product.
    pub(crate) fn resolve(&self, holder: Holder<'_>) -> Cow<'_, Self> {
        if !self.varies() {
            return Cow::Borrowed(self);
        }
        let mut parts = Vec::new();
        for part in &self.parts {
            match part {
                PathPart::Group(group) if holder.keeps(group) => {
                    for part in group {
                        push_resolved(&mut parts, part, holder);
                    }
                }
                PathPart::Group(_) => {}
                part => push_resolved(&mut parts, part, holder),
            }
        }
        if parts.is_empty() {
            parts.push(PathPart::Literal(String::new()));
        }
        Cow::Owned(Self {
            text: render(&parts),
            parts,
        })
    }

    /// Whether `holder`'s product's path has `{@labels}` written out in it.
    pub(crate) fn writes_labels(&self, holder: Holder<'_>) -> bool {
        let labels = |part: &PathPart| *part == PathPart::Placeholder(PathPlaceholder::Labels);
        holder.has(&PathPlaceholder::Labels)
            && self.parts.iter().any(|part| match part {
                PathPart::Group(group) => holder.keeps(group) && group.iter().any(labels),
                part => labels(part),
            })
    }

    /// Each dimension a `[...]` group names.
    pub(crate) fn group_dimensions(&self) -> impl Iterator<Item = &str> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                PathPart::Group(group) => Some(group),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                PathPart::Placeholder(PathPlaceholder::Dimension(name)) => Some(name.as_str()),
                _ => None,
            })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn parts(&self) -> &[PathPart] {
        &self.parts
    }

    /// The extension the template ends with: the text of its last file
    /// name after its final placeholder, from the first `.`, as `.nii.gz`
    /// in `sub-{sub}_T1w.nii.gz`. Values cannot add a `.`, as they are
    /// escaped, so only the template's own text can.
    pub fn extension(&self) -> Option<&str> {
        let Some(PathPart::Literal(tail)) = self.parts.last() else {
            return None;
        };
        let name = tail.rsplit('/').next().unwrap_or(tail);
        name.find('.').map(|dot| &name[dot..])
    }

    /// This template without `extension` at its end, or `None` when it does
    /// not end with it.
    #[must_use]
    pub(crate) fn without_extension(&self, extension: &str) -> Option<Self> {
        let mut parts = self.parts.clone();
        let Some(PathPart::Literal(tail)) = parts.last_mut() else {
            return None;
        };
        tail.truncate(tail.strip_suffix(extension)?.len());
        if tail.is_empty() {
            parts.pop();
        }
        Some(Self {
            text: self.text.strip_suffix(extension)?.to_owned(),
            parts,
        })
    }

    /// This template with `extension` added to its end.
    #[must_use]
    pub(crate) fn with_extension(&self, extension: &str) -> Self {
        let mut parts = self.parts.clone();
        match parts.last_mut() {
            Some(PathPart::Literal(tail)) => tail.push_str(extension),
            _ => parts.push(PathPart::Literal(extension.to_owned())),
        }
        Self {
            text: format!("{}{extension}", self.text),
            parts,
        }
    }

    /// This template with `{@product}` written out as `name`, so that an
    /// imported product keeps the path its own file gives it.
    #[must_use]
    pub(crate) fn with_product(&self, name: &str) -> Self {
        fn named(parts: &[PathPart], name: &str) -> Vec<PathPart> {
            parts
                .iter()
                .map(|part| match part {
                    PathPart::Placeholder(PathPlaceholder::Product) => {
                        PathPart::Literal(name.to_owned())
                    }
                    PathPart::Group(group) => PathPart::Group(named(group, name)),
                    part => part.clone(),
                })
                .collect()
        }
        let parts = named(&self.parts, name);
        Self {
            text: render(&parts),
            parts,
        }
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

/// Read a path template's parts: `[...]` groups, each holding text and
/// placeholders, between text and placeholders. `[[` and `]]` are literal
/// brackets, as `{{` and `}}` are literal braces.
fn parse_parts(text: &str) -> Result<Vec<PathPart>, String> {
    let mut parts = Vec::new();
    // The text since the last bracket, with `[[` and `]]` read.
    let mut segment = String::new();
    // Where the open group's `[` is.
    let mut open = None;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        match character {
            '[' | ']' if characters.peek().map(|&(_, next)| next) == Some(character) => {
                characters.next();
                segment.push(character);
            }
            '[' => {
                if open.is_some() {
                    return Err(format!(
                        "`[` inside `[...]` in `{text}`; a group cannot hold another"
                    ));
                }
                push_segment(&mut parts, &std::mem::take(&mut segment))?;
                open = Some(index);
            }
            ']' => {
                let start = open.take().ok_or_else(|| {
                    format!("unmatched `]` in `{text}`; write `]]` for a literal `]`")
                })?;
                let mut group = Vec::new();
                push_segment(&mut group, &std::mem::take(&mut segment))?;
                if !group.iter().any(|part| {
                    matches!(
                        part,
                        PathPart::Placeholder(
                            PathPlaceholder::Dimension(_)
                                | PathPlaceholder::Stage
                                | PathPlaceholder::Labels
                        )
                    )
                }) {
                    return Err(format!(
                        "`{}` names nothing that could be absent; remove the brackets",
                        &text[start..=index]
                    ));
                }
                parts.push(PathPart::Group(group));
            }
            character => segment.push(character),
        }
    }
    if open.is_some() {
        return Err(format!(
            "unclosed `[` in `{text}`; write `[[` for a literal `[`"
        ));
    }
    push_segment(&mut parts, &segment)?;
    if parts.is_empty() {
        parts.push(PathPart::Literal(String::new()));
    }
    Ok(parts)
}

/// Add the text and placeholders of `segment`, text between brackets.
fn push_segment(parts: &mut Vec<PathPart>, segment: &str) -> Result<(), String> {
    if segment.is_empty() {
        return Ok(());
    }
    for part in parse_template(segment)? {
        match part {
            Part::Literal(value) => push_literal(parts, &value),
            Part::Placeholder(name) => {
                parts.push(PathPart::Placeholder(PathPlaceholder::parse(name)?));
            }
        }
    }
    Ok(())
}

/// Add `value` to the text `parts` ends with, so that text is one part.
fn push_literal(parts: &mut Vec<PathPart>, value: &str) {
    match parts.last_mut() {
        Some(PathPart::Literal(tail)) => tail.push_str(value),
        _ => parts.push(PathPart::Literal(value.to_owned())),
    }
}

/// Add `part` of a group `holder`'s product keeps, or of no group, with
/// `{@labels}` written out when the product has dimensions.
fn push_resolved(parts: &mut Vec<PathPart>, part: &PathPart, holder: Holder<'_>) {
    match part {
        PathPart::Literal(value) => push_literal(parts, value),
        PathPart::Placeholder(PathPlaceholder::Labels) if holder.has(&PathPlaceholder::Labels) => {
            for (index, dimension) in holder.dimensions.iter().enumerate() {
                if index > 0 {
                    push_literal(parts, "_");
                }
                push_literal(parts, &format!("{}-", encode_component(dimension)));
                parts.push(PathPart::Placeholder(PathPlaceholder::Dimension(
                    dimension.clone(),
                )));
            }
        }
        part => parts.push(part.clone()),
    }
}

/// The text of `parts`, as a template would write them.
fn render(parts: &[PathPart]) -> String {
    let mut text = String::new();
    for part in parts {
        match part {
            PathPart::Literal(value) => {
                for character in value.chars() {
                    if matches!(character, '{' | '}' | '[' | ']') {
                        text.push(character);
                    }
                    text.push(character);
                }
            }
            PathPart::Placeholder(placeholder) => text.push_str(&placeholder.to_string()),
            PathPart::Group(group) => {
                text.push('[');
                text.push_str(&render(group));
                text.push(']');
            }
        }
    }
    text
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
/// declared dimension order, which `{@entities}` follows. `label` names the
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
    template: Cow<'p, PathTemplate>,
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
                            "path template for `{}` uses absent dimension `{dimension}`{}",
                            artifact.product,
                            PathPlaceholder::hint(dimension).unwrap_or_else(|| {
                                "; put it in `[...]` if only some products have it".to_owned()
                            })
                        ))
                        .focus(placeholder.to_string())
                    })?;
                    push_encoded(&mut relative, value);
                }
                // A product with dimensions has `{@labels}` written out.
                PathPart::Placeholder(PathPlaceholder::Labels) => {
                    return Err(error(format!(
                        "path template for `{}` uses `{}`, but `{}` has no dimensions; put it in `[...]`, as `[{}_]`",
                        artifact.product,
                        PathPlaceholder::Labels,
                        artifact.product,
                        PathPlaceholder::Labels
                    ))
                    .focus(PathPlaceholder::Labels.to_string()));
                }
                PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
            }
        }
        if let Some(reason) = unusable_path(&relative) {
            return Err(error(format!("{} {reason}: `{relative}`", label())));
        }
        Ok(relative)
    }
}

/// `product`'s path template with `{@product}` and `{@stage}` written out, as
/// every artifact of it shares them, for showing beside its declaration:
/// `derivatives/yield_table/{@entities}.csv`.
pub(crate) fn shown_path(pipeline: &Pipeline, product: &str) -> Option<String> {
    let path = ProductPath::new(pipeline, product).ok()?;
    let mut shown = String::new();
    for part in path.template.parts() {
        match part {
            PathPart::Literal(value) => shown.push_str(value),
            PathPart::Placeholder(PathPlaceholder::Product) => shown.push_str(&path.name),
            PathPart::Placeholder(PathPlaceholder::Stage) => {
                shown.push_str(path.stage.as_deref().unwrap_or("{@stage}"));
            }
            PathPart::Placeholder(placeholder) => shown.push_str(&placeholder.to_string()),
            PathPart::Group(_) => unreachable!("a product's template has its groups resolved"),
        }
    }
    Some(shown)
}

/// Add what `{@entities}` binds to: each dimension as `dimension=value`, in
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
            PathPart::Group(_) => {
                return Err(error(format!(
                    "discovery `{}` pattern cannot have an optional `[...]` part; every directory it finds has each dimension",
                    rule.name
                )));
            }
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
