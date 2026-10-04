//! A dataset's inputs: the rules that settle them, what they removed, and
//! the inventory of sources they leave.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::paths::PathTemplate;

use super::{owned_strings, DirectoryDiscovery, EntityBinding, Pipeline, PipelineIndex};

/// How a dataset's sources are found and filtered: directory discovery,
/// `exclude` and `require` rules, and where source files live. The input stage
/// reads these; job resolution never does.
#[derive(Clone, Debug, Default)]
pub struct InputRules {
    pub discoveries: Vec<DirectoryDiscovery>,
    /// `require` and conditional `exclude` rules, in declaration order.
    pub constraints: Vec<CoverageRule>,
    /// `exclude` rules, in declaration order, with each row of a file an
    /// `exclude from` line names in its place once the file is read.
    pub exclusions: Vec<Exclusion>,
    /// The files `exclude from` lines name, relative to the recipe's
    /// folder, with the line of each, until they are read.
    pub exclusion_files: Vec<(String, usize)>,
    /// Path rules for source products that the recipe, not the pipeline, sets.
    pub source_paths: BTreeMap<String, PathTemplate>,
    /// The recipe's `path:` line: the rule for each source with none of its
    /// own, in the pipeline or the recipe. It stays as written;
    /// [`InputRules::source_paths_for`] gives each source its rule.
    pub source_default: Option<PathTemplate>,
}

impl InputRules {
    pub fn is_empty(&self) -> bool {
        self.discoveries.is_empty()
            && self.constraints.is_empty()
            && self.exclusions.is_empty()
            && self.exclusion_files.is_empty()
            && self.source_paths.is_empty()
            && self.source_default.is_none()
    }

    /// Sources with no rule in the pipeline or recipe, except those whose
    /// paths follow another source through `beside`.
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

    /// The source paths the recipe names. A source written beside another
    /// takes that source's rule instead of having one of its own.
    pub fn named_source_paths(
        &self,
        _pipeline: &Pipeline,
    ) -> Cow<'_, BTreeMap<String, PathTemplate>> {
        Cow::Borrowed(&self.source_paths)
    }

    /// The recipe's named paths, plus its default for sources that need one.
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
            paths.insert(name.to_owned(), default.clone());
        }
        Cow::Owned(paths)
    }

    /// The discovery rule named `name`, if any.
    pub fn discovery(&self, name: &str) -> Option<&DirectoryDiscovery> {
        self.discoveries.iter().find(|rule| rule.name == name)
    }
}

/// An `exclude` rule: it removes every source artifact whose identity
/// includes each value it names, of its product or, when it names none, of
/// every product, with every discovered context that does too.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Exclusion {
    pub product: Option<String>,
    /// Each dimension and value it names, in the order written.
    pub values: Vec<(String, String)>,
    /// Why, from the comment on its line or a file's `reason` column.
    pub reason: Option<String>,
    /// Where it is written: `line 4` of the recipe, or `qc/excluded.csv row
    /// 3` for a row of a file an `exclude from` line names.
    pub origin: String,
}

impl Exclusion {
    /// Whether it removes the artifact of `product` with `entities`.
    pub fn matches(&self, product: &str, entities: &EntityBinding) -> bool {
        self.product.as_deref().is_none_or(|name| name == product) && self.within(entities)
    }

    /// Whether it removes the discovered context `binding`: only a rule that
    /// names no product removes contexts.
    pub fn matches_context(&self, binding: &EntityBinding) -> bool {
        self.product.is_none() && self.within(binding)
    }

    /// Whether `entities` has every value this rule names.
    fn within(&self, entities: &EntityBinding) -> bool {
        self.values
            .iter()
            .all(|(dimension, value)| entities.get(dimension) == Some(value.as_str()))
    }

    /// The values it names, as a binding.
    pub fn binding(&self) -> EntityBinding {
        self.values.iter().cloned().collect()
    }

    /// What it names, as written: `bold[sub=02,run=3]`, `[store=s07]`, or
    /// `testset`.
    pub fn pattern(&self) -> String {
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

impl fmt::Display for Exclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exclude {}", self.pattern())
    }
}

/// What the input stage left out of a dataset, and the rule that did: an
/// artifact of `product`, or, with no product, a group, every artifact
/// whose identity includes `entities`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Removal {
    pub product: Option<String>,
    pub entities: EntityBinding,
    /// The rule, as `exclude bold[run=3]` or `exclude [sub] where sessions
    /// count<2`.
    pub rule: String,
    /// Where the rule is written, when known: `line 4` of the recipe, or a
    /// line of a file.
    pub origin: Option<String>,
    pub reason: Option<String>,
    /// For a group a conditional `exclude` rule counted: how many it found.
    pub found: Option<usize>,
}

impl Removal {
    /// What was removed, as `bold[run=3,sub=02]` or `[sub=03]`, its
    /// dimensions in name order.
    pub fn identity(&self) -> String {
        self.identity_in(&[])
    }

    /// As [`Removal::identity`], with the dimensions `declared` names first,
    /// in that order.
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

    /// Whether a named `exclude` rule made it, rather than a conditional one.
    pub fn is_exclusion(&self) -> bool {
        self.rule.starts_with("exclude ") && !self.rule.contains("] where ")
    }
}

/// Add `[dimension=value,...]` to `text`.
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

/// A source record identifies a logical artifact, and may say where its file
/// is, relative to the dataset root, as the input stage found it.
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

    /// This record with its file at `path`, relative to the dataset root.
    #[must_use]
    pub fn at(self, path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            ..self
        }
    }
}

/// Supplied by a dataset indexer, a manifest, or the text fixture parser.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceInventory {
    pub artifacts: Vec<SourceRecord>,
    /// Observed contexts can expose missing artifacts even when no other source
    /// family has an artifact for that context.
    pub contexts: Vec<EntityBinding>,
    /// Bindings from each named directory discovery rule. These are also
    /// present in `contexts`, but retain their origin for coverage rules.
    pub discovered: BTreeMap<String, Vec<EntityBinding>>,
    /// Source path rules settled from a recipe, when the pipeline does not
    /// declare them. A .spitout carries each rule once for standalone DAGs.
    pub source_paths: BTreeMap<String, PathTemplate>,
    /// What the input stage left out, and why: a record, not a rule, so
    /// resolving jobs removes nothing more for it.
    pub removed: Vec<Removal>,
    /// The dataset root a `.spitout`'s `root` line names, as written:
    /// relative to the `.spitout`'s folder unless absolute.
    pub root: Option<PathBuf>,
}

/// A comparison of how many artifacts or contexts a group holds, as a
/// rule writes it after `count`: `=2`, `!=2`, `>=2`, `<=2`, `>2` or `<2`.
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
    /// Whether `found` artifacts or bindings meet the requirement.
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

    /// The comparison as a rule writes it: `count>=2`.
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

    /// The comparison that holds exactly when this one does not.
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

/// A `require` or conditional `exclude` rule over the groups of a dataset that `group_by`
/// forms, counting the artifacts of a source, or the contexts of a
/// discovery rule, in each: `product`.
///
/// A `require` rule fails a group unless its count holds and it has every
/// value in `values`. A conditional `exclude` rule removes a group when its count holds,
/// when it lacks a value in `values`, or when it has a value in `has`; it
/// names one of the three.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageRule {
    pub action: CoverageAction,
    pub product: String,
    pub group_by: Vec<String>,
    /// The count; a `require` rule without one needs at least one.
    pub count: Option<CountRequirement>,
    /// Entity values that must each be present in every group, such as
    /// `run=1,2`. Each listed dimension is checked on its own.
    pub values: BTreeMap<String, Vec<String>>,
    /// For `drop … has`: values any one of which removes a group.
    pub has: BTreeMap<String, Vec<String>>,
    /// The recipe line the rule is written on, when known.
    pub line: Option<usize>,
}

/// Reads as written: `exclude [sub] where sessions count<2`, or `require
/// [sub] where image has run=1,2`.
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

    /// Whether a group whose members have `bindings` passes a `require`
    /// rule, or is removed by a conditional `exclude` rule.
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
