//! Path templates: a rule's text read into parts, completed for a product,
//! and written back. Every step shares this: it knows the model, and nothing
//! about resolving or discovery.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::builtin::{ENTITIES, PRODUCT, STAGE};
use super::rule_text::{check_shapes, flatten, open_apart, parse_parts, push_literal, render};
use crate::span::Located;

use super::pattern::encode_component;
use super::shape::{shape_names, Shape};
use super::EntitiesFormat;

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
    /// `{@entities}`: every dimension, in the pipeline's order, as the
    /// nearest `default entities:` spells them. A product's template has it
    /// written out, unless the product has no dimensions or no spelling.
    Entities,
    /// `{@stage}`: the stage whose block holds the step.
    Stage,
    /// Any other name: a dimension the product declares, with the shape
    /// its values must have when a source's files are found, as in
    /// `{date:date}`. A shape narrows what a rule matches and changes no
    /// path it writes.
    Dimension(String, Option<Shape>),
}

impl PathPlaceholder {
    /// The built-in placeholder `{name}` writes, if any, with its `@`.
    fn built_in(name: &str) -> Option<Self> {
        match name {
            PRODUCT => Some(Self::Product),
            ENTITIES => Some(Self::Entities),
            STAGE => Some(Self::Stage),
            _ => None,
        }
    }

    /// The placeholder `{name}` is: a built-in as `@product`, or else a
    /// dimension, which takes no `@`.
    pub(super) fn parse(name: String) -> Result<Self, String> {
        match name.strip_prefix('@') {
            Some(built_in) if built_in.contains(':') => Err(format!(
                "`{{{name}}}`: a built-in placeholder takes no shape; write the dimension, as `sub-{{sub:digits}}`"
            )),
            Some("labels") => Err(
                "`{@labels}` is gone: spell `{@entities}` the same way, with `default entities: {@key}-{@value} separated \"_\"`".to_owned(),
            ),
            Some(_) => Self::built_in(&name).ok_or_else(|| {
                format!(
                    "unknown built-in placeholder `{{{name}}}`; path templates have `{{@product}}`, `{{@entities}}` and `{{@stage}}`"
                )
            }),
            None => match name.split_once(':') {
                None => Ok(Self::Dimension(name, None)),
                Some((dimension, shape)) => {
                    let shape = Shape::from_name(shape).ok_or_else(|| {
                        format!(
                            "unknown shape `{shape}` in `{{{name}}}`; the shapes are {}",
                            shape_names()
                        )
                    })?;
                    if dimension.is_empty() {
                        return Err(format!("`{{{name}}}` has a shape but no dimension"));
                    }
                    Ok(Self::Dimension(dimension.to_owned(), Some(shape)))
                }
            },
        }
    }

    /// What to write instead when a product has no dimension `name`, if
    /// `name` is a built-in written without its `@`.
    pub(crate) fn hint(name: &str) -> Option<String> {
        let meaning = match Self::built_in(&["@", name].concat())? {
            Self::Product => "the product's name",
            Self::Entities => "every dimension, as `default entities:` spells them",
            Self::Stage => "the stage that makes it",
            Self::Dimension(..) => return None,
        };
        Some(format!("; write `{{@{name}}}` for {meaning}"))
    }

    /// The name between the braces, with a built-in's `@`.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Product => PRODUCT,
            Self::Entities => ENTITIES,
            Self::Stage => STAGE,
            Self::Dimension(name, _) => name,
        }
    }
}

/// Reads as the placeholder is written, such as `{@stage}`.
impl fmt::Display for PathPlaceholder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimension(name, Some(shape)) => write!(f, "{{{name}:{shape}}}"),
            _ => write!(f, "{{{}}}", self.name()),
        }
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
    /// `*`: any text within one folder or file name, which binds nothing.
    /// A rule with one reads a source's files and writes no path.
    Wildcard,
    /// `{dimension=value}`: the value of `dimension` for every file the rule
    /// matches. It matches no text, and a rule with one writes no path.
    Fixed {
        dimension: String,
        value: String,
    },
}

/// What a product gives the placeholders of a path template: its
/// dimensions, in the pipeline's order, and whether a stage makes it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Holder<'a> {
    pub(crate) dimensions: &'a [String],
    pub(crate) entities_format: Option<&'a EntitiesFormat>,
    pub(crate) in_stage: bool,
}

impl Holder<'_> {
    /// Whether `placeholder` has a value for the product.
    fn has(self, placeholder: &PathPlaceholder) -> bool {
        match placeholder {
            PathPlaceholder::Product => true,
            PathPlaceholder::Entities => !self.dimensions.is_empty(),
            PathPlaceholder::Stage => self.in_stage,
            PathPlaceholder::Dimension(name, _) => self.dimensions.contains(name),
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

/// A path rule's template, parsed once when the rule is read. A source may
/// have several rules, and a file any of them matches is that source's: the
/// first is this template's own text and parts, and the rest are
/// `alternatives`, each with none of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathTemplate {
    pub(crate) text: String,
    parts: Vec<PathPart>,
    alternatives: Vec<PathTemplate>,
}

/// What `{@product}` writes for the product `name`: `alias::name` becomes
/// `alias.name`, since colons would go into file names.
pub(crate) fn product_text(name: &str) -> std::borrow::Cow<'_, str> {
    if name.contains("::") {
        std::borrow::Cow::Owned(name.replace("::", "."))
    } else {
        std::borrow::Cow::Borrowed(name)
    }
}

impl PathTemplate {
    /// Parse a template such as `derivatives/{@stage}/{@product}/{@entities}.mif`.
    pub fn parse(text: impl Into<String>) -> Result<Self, PathError> {
        let text = text.into();
        let parts = parse_parts(&text).map_err(error)?;
        check_shapes(&parts, &text).map_err(error)?;
        Ok(Self {
            text,
            parts,
            alternatives: Vec::new(),
        })
    }

    /// A source's rules, in the order written, as one template. `rules`
    /// holds at least one template, each of a single rule.
    pub(crate) fn several(mut rules: Vec<Self>) -> Self {
        let mut first = rules.remove(0);
        first.alternatives = rules;
        first
    }

    /// Each rule, in the order written. The first is this template, which
    /// carries the others too; read only its own text and parts.
    pub(crate) fn rules(&self) -> impl Iterator<Item = &Self> {
        std::iter::once(self).chain(&self.alternatives)
    }

    /// Whether the template has more than one rule.
    pub(crate) fn is_several(&self) -> bool {
        !self.alternatives.is_empty()
    }

    /// Whether a binding gives the one path this template writes for it: it
    /// has one rule, with no `*` and no fixed value. A source whose template
    /// does not keeps the path of each file discovery finds.
    pub(crate) fn writes_paths(&self) -> bool {
        !self.is_several() && self.reads_only().is_none()
    }

    /// The first part, of any rule, that only a rule reading a source's
    /// files may have: `*`, or a fixed value such as `{cohort=ctrl}`.
    pub(crate) fn reads_only(&self) -> Option<&PathPart> {
        fn find(parts: &[PathPart]) -> Option<&PathPart> {
            parts.iter().find_map(|part| match part {
                PathPart::Wildcard | PathPart::Fixed { .. } => Some(part),
                PathPart::Group(group) => find(group),
                _ => None,
            })
        }
        self.rules().find_map(|rule| find(&rule.parts))
    }

    /// Each dimension the rule binds: those it reads from a name and those
    /// it fixes, groups included.
    pub(crate) fn bound_dimensions(&self) -> BTreeSet<&str> {
        let mut all = Vec::new();
        flatten(&self.parts, &mut all);
        all.into_iter()
            .filter_map(|part| match part {
                PathPart::Placeholder(PathPlaceholder::Dimension(name, _)) => Some(name.as_str()),
                PathPart::Fixed { dimension, .. } => Some(dimension.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Fail when two placeholders of this template, with nothing between
    /// them, can both be any length: with `source`, any two dimensions
    /// without a shape of fixed length; else two with shapes of any length.
    /// `parse` checks the shapes of the template as written; a product whose
    /// `[...]` group is dropped has a different one, and only a source's
    /// files are read back into values, so this runs on `resolve`'s result.
    pub(crate) fn check_touching(&self, source: bool) -> Result<(), String> {
        self.rules().try_for_each(|rule| {
            let mut all = Vec::new();
            flatten(&rule.parts, &mut all);
            open_apart(&all, &rule.text, source)
        })
    }

    /// Whether the template has a `[...]` group, which each product
    /// resolves its own way.
    pub(crate) fn varies(&self) -> bool {
        self.rules().any(|rule| {
            rule.parts
                .iter()
                .any(|part| matches!(part, PathPart::Group(_)))
        })
    }

    /// This template as `holder`'s product has it: each group kept whole or
    /// dropped, and labels or custom entities written out as dimension
    /// placeholders. A pipeline without an `entities:` line leaves
    /// `{@entities}` in place, and binding it is an error.
    pub(crate) fn resolve(&self, holder: Holder<'_>) -> Cow<'_, Self> {
        if !self.varies()
            && (holder.entities_format.is_none()
                || !self.rules().any(|rule| {
                    rule.parts
                        .contains(&PathPart::Placeholder(PathPlaceholder::Entities))
                }))
        {
            return Cow::Borrowed(self);
        }
        Cow::Owned(self.map_rules(|rule| rule.resolve_one(holder)))
    }

    /// This template with `change` made to each rule.
    fn map_rules(&self, change: impl Fn(&Self) -> Self) -> Self {
        Self {
            alternatives: self.alternatives.iter().map(&change).collect(),
            ..change(self)
        }
    }

    /// This rule, without its alternatives, as [`PathTemplate::resolve`]
    /// gives it.
    fn resolve_one(&self, holder: Holder<'_>) -> Self {
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
        Self {
            text: render(&parts),
            parts,
            alternatives: Vec::new(),
        }
    }

    /// Whether the template names `{@entities}` outside a `[...]` group, so
    /// that only a product with dimensions can use it.
    pub(crate) fn needs_entities(&self) -> bool {
        self.rules().any(|rule| {
            rule.parts
                .contains(&PathPart::Placeholder(PathPlaceholder::Entities))
        })
    }

    /// Whether the template names `{@stage}` outside a `[...]` group, so
    /// that only a product made in a stage can use it.
    pub(crate) fn needs_stage(&self) -> bool {
        self.rules().any(|rule| {
            rule.parts
                .contains(&PathPart::Placeholder(PathPlaceholder::Stage))
        })
    }

    /// A dimension the template names outside a `[...]` group that is not
    /// one of `dimensions`, so that a product with only `dimensions` cannot
    /// use it.
    pub(crate) fn needs_dimension_outside(&self, dimensions: &[String]) -> Option<&str> {
        self.rules()
            .flat_map(|rule| &rule.parts)
            .find_map(|part| match part {
                PathPart::Placeholder(PathPlaceholder::Dimension(name, _))
                | PathPart::Fixed {
                    dimension: name, ..
                } if !dimensions.contains(name) => Some(name.as_str()),
                _ => None,
            })
    }

    /// Each placeholder with a shape, as `{date:date}`, and the dimension it
    /// shapes, groups and every rule included.
    pub(crate) fn shaped(&self) -> Vec<(&str, Shape)> {
        fn collect<'a>(parts: &'a [PathPart], found: &mut Vec<(&'a str, Shape)>) {
            for part in parts {
                match part {
                    PathPart::Placeholder(PathPlaceholder::Dimension(name, Some(shape))) => {
                        found.push((name, *shape));
                    }
                    PathPart::Group(group) => collect(group, found),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        for rule in self.rules() {
            collect(&rule.parts, &mut found);
        }
        found
    }

    /// Each dimension a `[...]` group of any rule names.
    pub(crate) fn group_dimensions(&self) -> impl Iterator<Item = &str> {
        self.rules()
            .flat_map(|rule| &rule.parts)
            .filter_map(|part| match part {
                PathPart::Group(group) => Some(group),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                PathPart::Placeholder(PathPlaceholder::Dimension(name, _)) => Some(name.as_str()),
                _ => None,
            })
    }

    /// The first rule's text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The first rule's parts.
    pub(crate) fn parts(&self) -> &[PathPart] {
        &self.parts
    }

    /// The extension the template's first rule ends with: the text of its last file
    /// name after its final placeholder, from the first `.`, as `.nii.gz`
    /// in `sub-{sub}_T1w.nii.gz`. Values cannot add a `.`, as they are
    /// escaped, so only the template's own text can.
    ///
    /// Keep in step with `EXTENSION_START` in `rules.rs`, which tells the
    /// user how the extension is read.
    pub fn extension(&self) -> Option<&str> {
        let Some(PathPart::Literal(tail)) = self.parts.last() else {
            return None;
        };
        let name = tail.rsplit('/').next().unwrap_or(tail);
        name.find('.').map(|dot| &name[dot..])
    }

    /// Whether every rule ends with `extension` after its last placeholder,
    /// as `sub-{sub}_acq-1.5T.nii.gz` ends with `.nii.gz`, though the
    /// extension [`PathTemplate::extension`] reads in it is `.5T.nii.gz`.
    pub(crate) fn ends_with(&self, extension: &str) -> bool {
        self.rules().all(|rule| rule.ends_with_one(extension))
    }

    /// Whether this rule, without its alternatives, ends with `extension`.
    fn ends_with_one(&self, extension: &str) -> bool {
        matches!(self.parts.last(), Some(PathPart::Literal(tail)) if tail.ends_with(extension))
    }

    /// This template without `extension` at the end of each rule, or `None`
    /// when a rule does not end with it.
    #[must_use]
    pub(crate) fn without_extension(&self, extension: &str) -> Option<Self> {
        Some(Self {
            alternatives: self
                .alternatives
                .iter()
                .map(|rule| rule.without_extension_one(extension))
                .collect::<Option<_>>()?,
            ..self.without_extension_one(extension)?
        })
    }

    /// This rule, without its alternatives, as
    /// [`PathTemplate::without_extension`] gives it.
    fn without_extension_one(&self, extension: &str) -> Option<Self> {
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
            alternatives: Vec::new(),
        })
    }

    /// This template with `extension` added to the end of each rule that
    /// does not end with it already.
    #[must_use]
    pub(crate) fn with_extension(&self, extension: &str) -> Self {
        self.map_rules(|rule| {
            let mut parts = rule.parts.clone();
            if rule.ends_with_one(extension) {
                return Self {
                    text: rule.text.clone(),
                    parts,
                    alternatives: Vec::new(),
                };
            }
            match parts.last_mut() {
                Some(PathPart::Literal(tail)) => tail.push_str(extension),
                _ => parts.push(PathPart::Literal(extension.to_owned())),
            }
            Self {
                text: format!("{}{extension}", rule.text),
                parts,
                alternatives: Vec::new(),
            }
        })
    }

    /// This template with `{@product}` written out as the text of the
    /// product `name`, so that an imported product keeps the path its own
    /// file gives it.
    #[must_use]
    pub(crate) fn with_product(&self, name: &str) -> Self {
        self.written_out(name, None, None)
    }

    /// This template with `{@product}` written out as the text of the
    /// product `name`, `{@stage}` as `stage`'s directories when it has one,
    /// and each dimension in `values` as its value, so that a file written
    /// beside `name`'s keeps the path `name`'s own artifact has.
    #[must_use]
    pub(crate) fn written_out(
        &self,
        name: &str,
        stage: Option<&str>,
        values: Option<&BTreeMap<String, String>>,
    ) -> Self {
        let name = &product_text(name);
        let written = |placeholder: &PathPlaceholder| match placeholder {
            PathPlaceholder::Product => Some(name.to_string()),
            PathPlaceholder::Stage => stage.map(str::to_owned),
            PathPlaceholder::Dimension(dimension, _) => {
                values?.get(dimension).map(|value| encode_component(value))
            }
            PathPlaceholder::Entities => None,
        };
        fn named(
            parts: &[PathPart],
            written: &impl Fn(&PathPlaceholder) -> Option<String>,
        ) -> Vec<PathPart> {
            parts
                .iter()
                .map(|part| match part {
                    PathPart::Placeholder(placeholder) => match written(placeholder) {
                        Some(text) => PathPart::Literal(text),
                        None => part.clone(),
                    },
                    PathPart::Group(group) => PathPart::Group(named(group, written)),
                    part @ (PathPart::Literal(_) | PathPart::Wildcard | PathPart::Fixed { .. }) => {
                        part.clone()
                    }
                })
                .collect()
        }
        self.map_rules(|rule| {
            let parts = named(&rule.parts, &written);
            Self {
                text: render(&parts),
                parts,
                alternatives: Vec::new(),
            }
        })
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

/// Add `part` of a group `holder`'s product keeps, or of no group, with
/// `{@entities}` written out when the pipeline has an `entities:` line.
fn push_resolved(parts: &mut Vec<PathPart>, part: &PathPart, holder: Holder<'_>) {
    match part {
        PathPart::Literal(value) => push_literal(parts, value),
        // A product with no dimensions keeps `{@entities}`, which binding
        // fails as it fails `{@stage}` outside every stage.
        PathPart::Placeholder(PathPlaceholder::Entities) if !holder.dimensions.is_empty() => {
            let Some(format) = holder.entities_format else {
                parts.push(part.clone());
                return;
            };
            for (index, dimension) in holder.dimensions.iter().enumerate() {
                if index > 0 {
                    push_literal(parts, &format.separator);
                }
                push_literal(parts, &format.prefix);
                let label = format.labels.get(dimension).unwrap_or(dimension);
                push_literal(parts, &encode_component(label));
                push_literal(parts, &format.between);
                parts.push(PathPart::Placeholder(PathPlaceholder::Dimension(
                    dimension.clone(),
                    None,
                )));
                push_literal(parts, &format.suffix);
            }
        }
        part => parts.push(part.clone()),
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
