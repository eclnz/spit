use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::builtin::{ENTITIES, PRODUCT, STAGE};
use super::rule_text::{check_shapes, flatten, open_apart, parse_parts, push_literal, render};
use crate::span::Located;

use super::pattern::encode_component;
use super::shape::{shape_names, Shape};
use super::EntitiesFormat;

crate::span::message_error!(PathProblem);

pub type PathError = Located<PathProblem>;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum PathPlaceholder {
    Product,
    Entities,
    Stage,
    Dimension(String, Option<Shape>),
}

impl PathPlaceholder {
    fn built_in(name: &str) -> Option<Self> {
        match name {
            PRODUCT => Some(Self::Product),
            ENTITIES => Some(Self::Entities),
            STAGE => Some(Self::Stage),
            _ => None,
        }
    }

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

    pub(crate) fn hint(name: &str) -> Option<String> {
        let meaning = match Self::built_in(&["@", name].concat())? {
            Self::Product => "the product's name",
            Self::Entities => "every dimension, as `default entities:` spells them",
            Self::Stage => "the stage that makes it",
            Self::Dimension(..) => return None,
        };
        Some(format!("; write `{{@{name}}}` for {meaning}"))
    }

    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Product => PRODUCT,
            Self::Entities => ENTITIES,
            Self::Stage => STAGE,
            Self::Dimension(name, _) => name,
        }
    }
}

impl fmt::Display for PathPlaceholder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimension(name, Some(shape)) => write!(f, "{{{name}:{shape}}}"),
            _ => write!(f, "{{{}}}", self.name()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PathPart {
    Literal(String),
    Placeholder(PathPlaceholder),
    Group(Vec<PathPart>),
    Wildcard,
    Fixed { dimension: String, value: String },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Holder<'a> {
    pub(crate) dimensions: &'a [String],
    pub(crate) entities_format: Option<&'a EntitiesFormat>,
    pub(crate) in_stage: bool,
}

impl Holder<'_> {
    fn has(self, placeholder: &PathPlaceholder) -> bool {
        match placeholder {
            PathPlaceholder::Product => true,
            PathPlaceholder::Entities => !self.dimensions.is_empty(),
            PathPlaceholder::Stage => self.in_stage,
            PathPlaceholder::Dimension(name, _) => self.dimensions.contains(name),
        }
    }

    fn keeps(self, group: &[PathPart]) -> bool {
        group.iter().all(|part| match part {
            PathPart::Placeholder(placeholder) => self.has(placeholder),
            _ => true,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathTemplate {
    pub(crate) text: String,
    parts: Vec<PathPart>,
    alternatives: Vec<PathTemplate>,
}

pub(crate) fn product_text(name: &str) -> std::borrow::Cow<'_, str> {
    if name.contains("::") {
        std::borrow::Cow::Owned(name.replace("::", "."))
    } else {
        std::borrow::Cow::Borrowed(name)
    }
}

impl PathTemplate {
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

    pub(crate) fn several(mut rules: Vec<Self>) -> Self {
        let mut first = rules.remove(0);
        first.alternatives = rules;
        first
    }

    pub(crate) fn rules(&self) -> impl Iterator<Item = &Self> {
        std::iter::once(self).chain(&self.alternatives)
    }

    pub(crate) fn is_several(&self) -> bool {
        !self.alternatives.is_empty()
    }

    pub(crate) fn writes_paths(&self) -> bool {
        !self.is_several() && self.reads_only().is_none()
    }

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

    pub(crate) fn check_touching(&self, source: bool) -> Result<(), String> {
        self.rules().try_for_each(|rule| {
            let mut all = Vec::new();
            flatten(&rule.parts, &mut all);
            open_apart(&all, &rule.text, source)
        })
    }

    pub(crate) fn varies(&self) -> bool {
        self.rules().any(|rule| {
            rule.parts
                .iter()
                .any(|part| matches!(part, PathPart::Group(_)))
        })
    }

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

    fn map_rules(&self, change: impl Fn(&Self) -> Self) -> Self {
        Self {
            alternatives: self.alternatives.iter().map(&change).collect(),
            ..change(self)
        }
    }

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

    pub(crate) fn needs_entities(&self) -> bool {
        self.rules().any(|rule| {
            rule.parts
                .contains(&PathPart::Placeholder(PathPlaceholder::Entities))
        })
    }

    pub(crate) fn needs_stage(&self) -> bool {
        self.rules().any(|rule| {
            rule.parts
                .contains(&PathPart::Placeholder(PathPlaceholder::Stage))
        })
    }

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

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn parts(&self) -> &[PathPart] {
        &self.parts
    }

    pub fn extension(&self) -> Option<&str> {
        let Some(PathPart::Literal(tail)) = self.parts.last() else {
            return None;
        };
        let name = tail.rsplit('/').next().unwrap_or(tail);
        name.find('.').map(|dot| &name[dot..])
    }

    pub(crate) fn ends_with(&self, extension: &str) -> bool {
        self.rules().all(|rule| rule.ends_with_one(extension))
    }

    fn ends_with_one(&self, extension: &str) -> bool {
        matches!(self.parts.last(), Some(PathPart::Literal(tail)) if tail.ends_with(extension))
    }

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

    #[must_use]
    pub(crate) fn with_product(&self, name: &str) -> Self {
        self.written_out(name, None, None)
    }

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

fn push_resolved(parts: &mut Vec<PathPart>, part: &PathPart, holder: Holder<'_>) {
    match part {
        PathPart::Literal(value) => push_literal(parts, value),
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

pub(crate) fn require_directory(root: &std::path::Path) -> Result<(), PathError> {
    if root.is_dir() {
        return Ok(());
    }
    Err(error(format!(
        "source root is not a directory: `{}`",
        root.display()
    )))
}
