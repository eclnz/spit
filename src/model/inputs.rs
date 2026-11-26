use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::template::{EntitiesFormat, Holder, PathGlob, PathTemplate};

use super::{owned_strings, ContextPattern, EntityBinding, Pipeline, PipelineIndex, ReviewRecord};

#[derive(Clone, Debug, Default)]
pub struct InputRules {
    pub context_paths: Vec<ContextPattern>,
    pub constraints: Vec<CoverageRule>,
    pub exclusions: Vec<Exclusion>,
    pub preferences: Vec<Preference>,
    pub reviews: Vec<ReviewFile>,
    pub source_paths: BTreeMap<String, PathTemplate>,
    pub source_default: Option<PathTemplate>,
    pub source_entities: Option<EntitiesFormat>,
}

impl InputRules {
    pub fn is_empty(&self) -> bool {
        self.context_paths.is_empty()
            && self.constraints.is_empty()
            && self.exclusions.is_empty()
            && self.preferences.is_empty()
            && self.reviews.is_empty()
            && self.source_paths.is_empty()
            && self.source_default.is_none()
            && self.source_entities.is_none()
    }

    pub fn defaulted_sources<'p>(&self, pipeline: &'p Pipeline) -> Vec<&'p str> {
        if self.source_default.is_none() {
            return Vec::new();
        }
        let index = PipelineIndex::new(pipeline);
        pipeline
            .products
            .iter()
            .filter(|product| {
                index.is_source(&product.name)
                    && product.beside.is_none()
                    && !pipeline.product_paths.contains_key(&product.name)
                    && !self.source_paths.contains_key(&product.name)
            })
            .map(|product| product.name.as_str())
            .collect()
    }

    pub fn named_source_paths(
        &self,
        pipeline: &Pipeline,
    ) -> Cow<'_, BTreeMap<String, PathTemplate>> {
        if self.source_entities.is_none() {
            return Cow::Borrowed(&self.source_paths);
        }
        Cow::Owned(
            self.source_paths
                .iter()
                .map(|(name, template)| (name.clone(), self.spelled(pipeline, name, template)))
                .collect(),
        )
    }

    fn spelled(&self, pipeline: &Pipeline, source: &str, template: &PathTemplate) -> PathTemplate {
        let Some(format) = &self.source_entities else {
            return template.clone();
        };
        let dimensions = pipeline
            .products
            .iter()
            .find(|product| product.name == source)
            .map_or(&[][..], |product| product.dimensions.as_slice());
        template
            .resolve(Holder {
                dimensions,
                entities_format: Some(format),
                in_stage: false,
            })
            .into_owned()
    }

    pub fn source_paths_for(&self, pipeline: &Pipeline) -> Cow<'_, BTreeMap<String, PathTemplate>> {
        let named = self.named_source_paths(pipeline);
        let defaulted = self.defaulted_sources(pipeline);
        let Some(default) = self
            .source_default
            .as_ref()
            .filter(|_| !defaulted.is_empty())
        else {
            return named;
        };
        let mut paths = named.into_owned();
        for name in defaulted {
            paths.insert(name.to_owned(), self.spelled(pipeline, name, default));
        }
        Cow::Owned(paths)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewFile {
    pub review: String,
    pub file: String,
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Exclusion {
    pub product: Option<String>,
    pub values: Vec<(String, String)>,
    pub path: Option<PathGlob>,
    pub reason: Option<String>,
    pub origin: String,
}

impl Exclusion {
    pub fn matches(&self, product: &str, entities: &EntityBinding, file: Option<&str>) -> bool {
        match &self.path {
            Some(path) => file.is_some_and(|file| path.covers(file)),
            None => {
                self.product.as_deref().is_none_or(|name| name == product) && self.within(entities)
            }
        }
    }

    pub fn matches_context(&self, binding: &EntityBinding) -> bool {
        self.product.is_none() && self.path.is_none() && self.within(binding)
    }

    fn within(&self, entities: &EntityBinding) -> bool {
        self.values
            .iter()
            .all(|(dimension, value)| entities.get(dimension) == Some(value.as_str()))
    }

    pub fn binding(&self) -> EntityBinding {
        self.values.iter().cloned().collect()
    }

    pub fn pattern(&self) -> String {
        if let Some(path) = &self.path {
            return format!("path {path}");
        }
        let mut text = self.product.clone().unwrap_or_default();
        if !self.values.is_empty() || self.product.is_none() {
            let pairs = self
                .values
                .iter()
                .map(|(dimension, value)| (dimension.as_str(), value.as_str()));
            push_bindings(&mut text, pairs);
        }
        text
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preference {
    pub path: PathGlob,
    pub reason: Option<String>,
    pub origin: String,
}

impl fmt::Display for Preference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "prefer path {}", self.path)
    }
}

impl fmt::Display for Exclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exclude {}", self.pattern())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Removal {
    pub product: Option<String>,
    pub entities: EntityBinding,
    pub rule: String,
    pub origin: Option<String>,
    pub reason: Option<String>,
    pub found: Option<usize>,
}

impl Removal {
    pub fn identity(&self) -> String {
        self.identity_in(&[])
    }

    pub fn identity_in(&self, declared: &[String]) -> String {
        let mut pairs: Vec<_> = self.entities.iter().collect();
        pairs.sort_by_key(|(dimension, _)| {
            declared
                .iter()
                .position(|name| name == dimension)
                .unwrap_or(usize::MAX)
        });
        let mut text = self.product.clone().unwrap_or_default();
        if !pairs.is_empty() || self.product.is_none() {
            push_bindings(&mut text, pairs.into_iter());
        }
        text
    }

    pub fn is_exclusion(&self) -> bool {
        self.rule.starts_with("exclude ") && !self.rule.contains("] where ")
    }
}

fn push_bindings<'a>(text: &mut String, pairs: impl Iterator<Item = (&'a str, &'a str)>) {
    text.push('[');
    for (index, (dimension, value)) in pairs.enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(dimension);
        text.push('=');
        text.push_str(value);
    }
    text.push(']');
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceRecord {
    pub product: String,
    pub entities: EntityBinding,
    pub path: Option<String>,
}

impl SourceRecord {
    pub fn new(product: impl Into<String>, entities: EntityBinding) -> Self {
        Self {
            product: product.into(),
            entities,
            path: None,
        }
    }

    #[must_use]
    pub fn at(self, path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            ..self
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceInventory {
    pub artifacts: Vec<SourceRecord>,
    pub contexts: Vec<EntityBinding>,
    pub source_paths: BTreeMap<String, PathTemplate>,
    pub removed: Vec<Removal>,
    pub review_files: Vec<ReviewRecord>,
    pub root: Option<PathBuf>,
    pub output: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountRequirement {
    Exactly(usize),
    NotExactly(usize),
    AtLeast(usize),
    AtMost(usize),
    MoreThan(usize),
    FewerThan(usize),
}

impl CountRequirement {
    pub fn allows(&self, found: usize) -> bool {
        match *self {
            Self::Exactly(count) => found == count,
            Self::NotExactly(count) => found != count,
            Self::AtLeast(count) => found >= count,
            Self::AtMost(count) => found <= count,
            Self::MoreThan(count) => found > count,
            Self::FewerThan(count) => found < count,
        }
    }

    pub fn as_written(&self) -> String {
        let (op, count) = match *self {
            Self::Exactly(count) => ("=", count),
            Self::NotExactly(count) => ("!=", count),
            Self::AtLeast(count) => (">=", count),
            Self::AtMost(count) => ("<=", count),
            Self::MoreThan(count) => (">", count),
            Self::FewerThan(count) => ("<", count),
        };
        format!("count{op}{count}")
    }

    pub fn negated(&self) -> Self {
        match *self {
            Self::Exactly(count) => Self::NotExactly(count),
            Self::NotExactly(count) => Self::Exactly(count),
            Self::AtLeast(count) => Self::FewerThan(count),
            Self::AtMost(count) => Self::MoreThan(count),
            Self::MoreThan(count) => Self::AtMost(count),
            Self::FewerThan(count) => Self::AtLeast(count),
        }
    }
}

impl fmt::Display for CountRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exactly(count) => write!(f, "exactly {count}"),
            Self::NotExactly(count) => write!(f, "other than {count}"),
            Self::AtLeast(count) => write!(f, "at least {count}"),
            Self::AtMost(count) => write!(f, "at most {count}"),
            Self::MoreThan(count) => write!(f, "more than {count}"),
            Self::FewerThan(count) => write!(f, "fewer than {count}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageRule {
    pub action: CoverageAction,
    pub product: String,
    pub group_by: Vec<String>,
    pub count: Option<CountRequirement>,
    pub values: BTreeMap<String, Vec<String>>,
    pub has: BTreeMap<String, Vec<String>>,
    pub line: Option<usize>,
}

impl fmt::Display for CoverageRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let clause = |values: &BTreeMap<String, Vec<String>>| {
            values
                .iter()
                .map(|(dimension, values)| format!("{dimension}={}", values.join(",")))
                .collect::<Vec<_>>()
                .join(" ")
        };
        match self.action {
            CoverageAction::Require => {
                write!(
                    f,
                    "require [{}] where {}",
                    self.group_by.join(", "),
                    self.product
                )?;
                if let Some(count) = self.count {
                    write!(f, " {}", count.as_written())?;
                }
                if !self.values.is_empty() {
                    write!(f, " has {}", clause(&self.values))?;
                }
                Ok(())
            }
            CoverageAction::Drop => {
                write!(
                    f,
                    "exclude [{}] where {}",
                    self.group_by.join(", "),
                    self.product
                )?;
                if let Some(count) = self.count {
                    write!(f, " {}", count.as_written())?;
                }
                if !self.values.is_empty() {
                    write!(f, " missing {}", clause(&self.values))?;
                }
                if !self.has.is_empty() {
                    write!(f, " has {}", clause(&self.has))?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoverageAction {
    #[default]
    Require,
    Drop,
}

impl CoverageRule {
    pub fn new(
        product: impl Into<String>,
        group_by: impl IntoIterator<Item = impl AsRef<str>>,
        count: CountRequirement,
    ) -> Self {
        Self {
            action: CoverageAction::Require,
            product: product.into(),
            group_by: owned_strings(group_by),
            count: Some(count),
            values: BTreeMap::new(),
            has: BTreeMap::new(),
            line: None,
        }
    }

    pub fn holds_for(&self, bindings: &[&EntityBinding]) -> bool {
        let lacks = |values: &BTreeMap<String, Vec<String>>| {
            values.iter().any(|(dimension, listed)| {
                listed.iter().any(|value| {
                    !bindings
                        .iter()
                        .any(|binding| binding.get(dimension) == Some(value.as_str()))
                })
            })
        };
        match self.action {
            CoverageAction::Require => {
                self.count
                    .unwrap_or(CountRequirement::AtLeast(1))
                    .allows(bindings.len())
                    && !lacks(&self.values)
            }
            CoverageAction::Drop => {
                let has = self.has.iter().any(|(dimension, listed)| {
                    bindings.iter().any(|binding| {
                        binding
                            .get(dimension)
                            .is_some_and(|found| listed.iter().any(|value| value == found))
                    })
                });
                self.count.is_some_and(|count| count.allows(bindings.len()))
                    || lacks(&self.values)
                    || has
            }
        }
    }

    #[must_use]
    pub fn requiring(
        mut self,
        dimension: impl Into<String>,
        values: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.values.insert(dimension.into(), owned_strings(values));
        self
    }
}
