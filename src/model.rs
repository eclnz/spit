use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::command::CommandTemplate;
use crate::error::{DefinitionSubject, ResolveError};
use crate::paths::PathTemplate;
use crate::types::TypeExpr;

pub type ArtifactType = TypeExpr;

/// The value an artifact has for each of its product's dimensions.
#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct EntityBinding(BTreeMap<String, String>);

impl From<BTreeMap<String, String>> for EntityBinding {
    fn from(values: BTreeMap<String, String>) -> Self {
        Self(values)
    }
}

impl FromIterator<(String, String)> for EntityBinding {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(values: I) -> Self {
        Self(values.into_iter().collect())
    }
}

impl EntityBinding {
    /// The value bound to `dimension`, if any.
    pub fn get(&self, dimension: &str) -> Option<&str> {
        self.0.get(dimension).map(String::as_str)
    }

    /// Whether `dimension` has a value.
    pub fn binds(&self, dimension: &str) -> bool {
        self.0.contains_key(dimension)
    }

    /// Each dimension and its value, in dimension name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(dimension, value)| (dimension.as_str(), value.as_str()))
    }

    /// The dimensions with a value, in name order.
    pub fn dimensions(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// How many dimensions have a value.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Add `other`'s values, replacing any this has for the same dimensions.
    pub(crate) fn extend(&mut self, other: &Self) {
        self.0.extend(
            other
                .0
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }

    pub fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(dimension, value)| (dimension.to_owned(), value.to_owned()))
                .collect(),
        )
    }

    pub fn without(&self, dimension: &str) -> Self {
        let mut values = self.0.clone();
        values.remove(dimension);
        Self(values)
    }

    pub fn matches_shared(&self, other: &Self) -> bool {
        self.0
            .iter()
            .all(|(dimension, value)| other.0.get(dimension).is_none_or(|other| other == value))
    }

    /// Keep only `dimensions`, or `None` if one of them is unbound.
    pub fn project(&self, dimensions: &[String]) -> Option<Self> {
        dimensions
            .iter()
            .map(|dimension| {
                let value = self.0.get(dimension)?;
                Some((dimension.clone(), value.clone()))
            })
            .collect::<Option<_>>()
            .map(Self)
    }

    /// Compare values dimension by dimension in `dimensions` order, reading
    /// runs of digits as numbers, so `run=2` sorts before `run=10`.
    pub fn cmp_in(&self, other: &Self, dimensions: &[String]) -> Ordering {
        dimensions
            .iter()
            .map(
                |dimension| match (self.0.get(dimension), other.0.get(dimension)) {
                    (Some(left), Some(right)) => natural_cmp(left, right),
                    (left, right) => left.cmp(&right),
                },
            )
            .find(|ordering| ordering.is_ne())
            .unwrap_or_else(|| self.cmp(other))
    }
}

/// Order text as people read it: runs of digits compare by numeric value.
pub(crate) fn natural_cmp(left: &str, right: &str) -> Ordering {
    let (mut left_rest, mut right_rest) = (left, right);
    loop {
        match (left_rest.chars().next(), right_rest.chars().next()) {
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let (a_digits, a_tail) = split_digits(left_rest);
                let (b_digits, b_tail) = split_digits(right_rest);
                let a_number = a_digits.trim_start_matches('0');
                let b_number = b_digits.trim_start_matches('0');
                let ordering = a_number
                    .len()
                    .cmp(&b_number.len())
                    .then_with(|| a_number.cmp(b_number));
                if ordering.is_ne() {
                    return ordering;
                }
                (left_rest, right_rest) = (a_tail, b_tail);
            }
            (Some(a), Some(b)) => {
                if a != b {
                    return a.cmp(&b);
                }
                left_rest = &left_rest[a.len_utf8()..];
                right_rest = &right_rest[b.len_utf8()..];
            }
        }
    }
}

fn split_digits(text: &str) -> (&str, &str) {
    let end = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

impl fmt::Display for EntityBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<_> = self.0.iter().map(|(k, v)| format!("{k}={v}")).collect();
        f.write_str(&parts.join(","))
    }
}

/// An artifact as `product[dimension=value,...]`, its entities in the order
/// given.
pub(crate) fn identity<'a>(
    product: &str,
    entities: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let bindings: Vec<_> = entities
        .into_iter()
        .map(|(dimension, value)| format!("{dimension}={value}"))
        .collect();
    format!("{product}[{}]", bindings.join(","))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductDef {
    pub name: String,
    pub artifact_type: ArtifactType,
    pub dimensions: Vec<String>,
}

impl ProductDef {
    pub fn new(
        name: impl Into<String>,
        artifact_type: ArtifactType,
        dimensions: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            dimensions: owned_strings(dimensions),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ArtifactInstance {
    pub product: String,
    pub artifact_type: ArtifactType,
    pub entities: EntityBinding,
}

/// An artifact's identity: its product and entity bindings, ignoring its type.
pub type ArtifactKey = (String, EntityBinding);

impl ArtifactInstance {
    pub fn key(&self) -> ArtifactKey {
        (self.product.clone(), self.entities.clone())
    }

    pub fn new(
        product: impl Into<String>,
        artifact_type: ArtifactType,
        entities: EntityBinding,
    ) -> Self {
        Self {
            product: product.into(),
            artifact_type,
            entities,
        }
    }
}

impl fmt::Display for ArtifactInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]", self.product, self.entities)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cardinality {
    One,
    Many,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputPort {
    pub name: String,
    pub artifact_type: ArtifactType,
    pub cardinality: Cardinality,
}

impl InputPort {
    pub fn one(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::One,
        }
    }

    pub fn many(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::Many,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShapeRule {
    /// The input with every dimension the others join on drives one job per
    /// artifact, and the outputs keep its dimensions.
    Preserve,
    /// The one many-valued input groups by the dimension named in its `vary`
    /// binding; any single-artifact inputs are matched to each group.
    Aggregate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPort {
    pub name: String,
    pub artifact_type: ArtifactType,
}

impl OutputPort {
    pub fn new(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
        }
    }
}

/// The port name of an operation's only, unnamed output.
pub(crate) const DEFAULT_OUTPUT: &str = "output";

/// A placeholder a command has without its operation naming the port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultPort {
    /// `{input}`: an operation's only input, when unnamed.
    Input,
    /// `{input1}`, `{input2}`, ...: the unnamed inputs of an operation with
    /// several, numbered from 1.
    InputAt(usize),
    /// `{inputs}`: an operation's only input when that is a many input,
    /// whatever its name.
    Inputs,
}

impl DefaultPort {
    /// The name SPIT gives the unnamed input at `index` of `count` inputs.
    pub fn for_input(index: usize, count: usize) -> Self {
        if count == 1 {
            Self::Input
        } else {
            Self::InputAt(index + 1)
        }
    }

    /// The name between the braces.
    pub fn name(self) -> String {
        match self {
            Self::Input => "input".to_owned(),
            Self::InputAt(number) => format!("input{number}"),
            Self::Inputs => "inputs".to_owned(),
        }
    }
}

/// Reads as the placeholder is written, such as `{input2}`.
impl fmt::Display for DefaultPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}}}", self.name())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDef {
    pub name: String,
    pub inputs: Vec<InputPort>,
    /// Every artifact one job writes, in declaration order.
    pub outputs: Vec<OutputPort>,
    pub shape_rule: ShapeRule,
    /// An optional declared dimension consumed by an aggregate operation.
    pub aggregated_dimension: Option<String>,
    /// The fewest artifacts the many input accepts in one job.
    pub minimum_collection: Option<usize>,
}

impl OperationDef {
    /// An operation with one output, named `output`.
    pub fn new(
        name: impl Into<String>,
        inputs: Vec<InputPort>,
        output_type: ArtifactType,
        shape_rule: ShapeRule,
    ) -> Self {
        Self::with_outputs(
            name,
            inputs,
            vec![OutputPort::new(DEFAULT_OUTPUT, output_type)],
            shape_rule,
        )
    }

    pub fn with_outputs(
        name: impl Into<String>,
        inputs: Vec<InputPort>,
        outputs: Vec<OutputPort>,
        shape_rule: ShapeRule,
    ) -> Self {
        Self {
            name: name.into(),
            inputs,
            outputs,
            shape_rule,
            aggregated_dimension: None,
            minimum_collection: None,
        }
    }

    #[must_use]
    pub fn aggregating(mut self, dimension: impl Into<String>) -> Self {
        self.aggregated_dimension = Some(dimension.into());
        self
    }

    #[must_use]
    pub fn at_least(mut self, minimum: usize) -> Self {
        self.minimum_collection = Some(minimum);
        self
    }
}

/// How a call binds one product to an operation input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputBinding {
    pub product: String,
    /// `@ vary(dimension)`: collect the artifacts that differ in this dimension.
    pub vary: Option<String>,
    /// `@ where(dimension=value, ...)`: keep only artifacts with these values.
    /// A pinned dimension no longer takes part in matching.
    pub pinned: BTreeMap<String, String>,
    /// `@ same(dimension, ...)`: match the job on these dimensions only. Any
    /// other dimension must leave exactly one artifact for each job.
    pub same: Option<Vec<String>>,
    /// `@ each(dimension, ...)`: broadcast the input over these dimensions.
    /// The step runs once per value of them found in the product, and its
    /// outputs gain them.
    pub each: Vec<String>,
}

impl InputBinding {
    pub fn product(name: impl Into<String>) -> Self {
        Self {
            product: name.into(),
            ..Self::default()
        }
    }

    pub fn vary(product: impl Into<String>, dimension: impl Into<String>) -> Self {
        Self {
            vary: Some(dimension.into()),
            ..Self::product(product)
        }
    }

    #[must_use]
    pub fn pin(mut self, dimension: impl Into<String>, value: impl Into<String>) -> Self {
        self.pinned.insert(dimension.into(), value.into());
        self
    }

    #[must_use]
    pub fn same_on(mut self, dimensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.same = Some(owned_strings(dimensions));
        self
    }

    #[must_use]
    pub fn each_of(mut self, dimensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.each = owned_strings(dimensions);
        self
    }

    pub fn product_name(&self) -> &str {
        &self.product
    }

    /// Whether any `@` selector is present.
    pub fn has_selectors(&self) -> bool {
        self.vary.is_some()
            || !self.pinned.is_empty()
            || self.same.is_some()
            || !self.each.is_empty()
    }

    /// The product's dimensions that remain after `where` pins some of them.
    pub fn free_dimensions(&self, dimensions: &[String]) -> Vec<String> {
        dimensions
            .iter()
            .filter(|dimension| !self.pinned.contains_key(*dimension))
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub operation: String,
    /// Bindings correspond to operation ports in declaration order.
    pub inputs: Vec<InputBinding>,
    /// Products, one per operation output port in declaration order.
    pub outputs: Vec<String>,
    /// The stage whose block holds this step, if any.
    pub stage: Option<String>,
}

impl Invocation {
    pub fn new(
        operation: impl Into<String>,
        inputs: Vec<InputBinding>,
        output_product: impl Into<String>,
    ) -> Self {
        Self::with_outputs(operation, inputs, [output_product])
    }

    pub fn with_outputs(
        operation: impl Into<String>,
        inputs: Vec<InputBinding>,
        outputs: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            operation: operation.into(),
            inputs,
            outputs: outputs.into_iter().map(Into::into).collect(),
            stage: None,
        }
    }

    #[must_use]
    pub fn in_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    /// The first output, which names the step in diagnostics.
    pub fn output_product(&self) -> &str {
        self.outputs.first().map_or("", String::as_str)
    }
}

/// What a command line does for its operation's jobs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandRole {
    /// Produce the job's outputs.
    Run,
    /// Check the job's inputs before it runs, such as their headers or grids;
    /// a nonzero exit stops the script.
    Verify,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDef {
    pub operation: String,
    pub template: CommandTemplate,
    pub role: CommandRole,
}

impl CommandDef {
    pub fn new(operation: impl Into<String>, template: CommandTemplate) -> Self {
        Self {
            operation: operation.into(),
            template,
            role: CommandRole::Run,
        }
    }

    pub fn verify(operation: impl Into<String>, template: CommandTemplate) -> Self {
        Self {
            role: CommandRole::Verify,
            ..Self::new(operation, template)
        }
    }
}

/// A named group of steps, such as preprocessing or analysis. A stage owns
/// the products its steps assign; operations stay global. A nested stage's
/// name is its path, as in `preprocess/denoise`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageDef {
    pub name: String,
    /// The default path rule for the products of this stage and the stages
    /// nested in it that set none, in place of the pipeline's default.
    pub path_template: Option<PathTemplate>,
}

/// A directory pattern that discovers concrete entity bindings under a root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryDiscovery {
    pub name: String,
    pub dimensions: Vec<String>,
    pub template: PathTemplate,
}

impl StageDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path_template: None,
        }
    }
}

/// The logical pipeline: what to make from which sources. It says nothing
/// about how a dataset's sources are found or filtered; see [`InputRules`].
#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    pub products: Vec<ProductDef>,
    pub operations: Vec<OperationDef>,
    pub invocations: Vec<Invocation>,
    pub commands: Vec<CommandDef>,
    pub path_template: Option<PathTemplate>,
    pub product_paths: BTreeMap<String, PathTemplate>,
    /// Stages in declaration order.
    pub stages: Vec<StageDef>,
}

impl Pipeline {
    /// The stage of the step that produces `product`; `None` for a source or
    /// a step outside every stage.
    pub fn stage_of(&self, product: &str) -> Option<&str> {
        self.invocations
            .iter()
            .find(|invocation| invocation.outputs.iter().any(|output| output == product))
            .and_then(|invocation| invocation.stage.as_deref())
    }

    /// The path template `product` uses: its own rule, else its stage's
    /// default, else the pipeline's default.
    pub fn path_template_for(&self, product: &str) -> Option<&PathTemplate> {
        self.product_paths
            .get(product)
            .or_else(|| self.stage_path_template(product))
            .or(self.path_template.as_ref())
    }

    /// The default path rule of the stage that produces `product`, or of the
    /// nearest stage around it that sets one, with the stage that sets it.
    pub fn stage_path_rule(&self, product: &str) -> Option<(&str, &PathTemplate)> {
        let stage = self.stage_of(product)?;
        stage_and_parents(stage).find_map(|name| {
            self.stages
                .iter()
                .find(|candidate| candidate.name == name)
                .and_then(|stage| Some((stage.name.as_str(), stage.path_template.as_ref()?)))
        })
    }

    pub fn stage_path_template(&self, product: &str) -> Option<&PathTemplate> {
        self.stage_path_rule(product).map(|(_, template)| template)
    }

    /// Whether `product` is a source family, which no step produces.
    pub fn is_source(&self, product: &str) -> bool {
        self.products
            .iter()
            .any(|declared| declared.name == product)
            && !self
                .invocations
                .iter()
                .any(|invocation| invocation.outputs.iter().any(|output| output == product))
    }

    /// Each record of `inventory` as an artifact, by product and in order,
    /// after checking that it names a source, binds exactly its dimensions,
    /// and appears once.
    pub fn source_artifacts(
        &self,
        inventory: &SourceInventory,
    ) -> Result<BTreeMap<String, Vec<ArtifactInstance>>, ResolveError> {
        // Looked up once, not once per record; the first declaration of a
        // name wins, as when searching.
        let mut products = BTreeMap::new();
        for product in &self.products {
            products.entry(product.name.as_str()).or_insert(product);
        }
        let produced: BTreeSet<_> = self
            .invocations
            .iter()
            .flat_map(|invocation| &invocation.outputs)
            .map(String::as_str)
            .collect();
        let mut artifacts: BTreeMap<String, Vec<ArtifactInstance>> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for record in &inventory.artifacts {
            let product = products.get(record.product.as_str()).ok_or_else(|| {
                ResolveError::UnknownProduct {
                    name: record.product.clone(),
                }
            })?;
            if produced.contains(record.product.as_str()) {
                return Err(ResolveError::InvalidDefinition {
                    subject: DefinitionSubject::Product(record.product.clone()),
                    detail: format!(
                        "product `{}` cannot be both a source family and an invocation output",
                        record.product
                    ),
                });
            }
            let source = ArtifactInstance {
                product: record.product.clone(),
                artifact_type: product.artifact_type.clone(),
                entities: record.entities.clone(),
            };
            let actual: BTreeSet<_> = record.entities.0.keys().collect();
            let expected: BTreeSet<_> = product.dimensions.iter().collect();
            if actual != expected {
                return Err(ResolveError::InvalidDefinition {
                    subject: DefinitionSubject::Source(record.clone()),
                    detail: format!(
                        "source `{source}` must bind exactly the dimensions of product `{}`: [{}]",
                        product.name,
                        product.dimensions.join(", ")
                    ),
                });
            }
            if !seen.insert(source.key()) {
                return Err(ResolveError::DuplicateSourceArtifact { artifact: source });
            }
            artifacts
                .entry(source.product.clone())
                .or_default()
                .push(source);
        }
        for (name, family) in &mut artifacts {
            if let Some(product) = products.get(name.as_str()) {
                product.sort_family(family);
            }
        }
        Ok(artifacts)
    }
}

impl ProductDef {
    /// Order this product's artifacts by its declared dimensions, reading
    /// numbers as numbers.
    pub fn sort_family(&self, family: &mut [ArtifactInstance]) {
        family.sort_by(|left, right| left.entities.cmp_in(&right.entities, &self.dimensions));
    }
}

/// How a dataset's sources are found and filtered: directory discovery,
/// `require` and `skip` rules, and where source files live. The input stage
/// reads these; job resolution never does.
#[derive(Clone, Debug, Default)]
pub struct InputRules {
    pub discoveries: Vec<DirectoryDiscovery>,
    /// `require` and `skip` rules, in declaration order.
    pub constraints: Vec<CoverageRule>,
    /// Path rules for source products that the recipe, not the pipeline, sets.
    pub source_paths: BTreeMap<String, PathTemplate>,
}

impl InputRules {
    pub fn is_empty(&self) -> bool {
        self.discoveries.is_empty() && self.constraints.is_empty() && self.source_paths.is_empty()
    }

    /// The discovery rule named `name`, if any.
    pub fn discovery(&self, name: &str) -> Option<&DirectoryDiscovery> {
        self.discoveries.iter().find(|rule| rule.name == name)
    }
}

/// A stage's full name, then each stage around it: `a/b/c`, `a/b`, `a`.
pub(crate) fn stage_and_parents(stage: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(stage), |name| {
        name.rsplit_once('/').map(|(parent, _)| parent)
    })
}

/// Whether `stage` is `outer` or a stage nested inside it.
pub fn stage_within(stage: &str, outer: &str) -> bool {
    stage
        .strip_prefix(outer)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CountRequirement {
    Exactly(usize),
    AtLeast(usize),
}

impl CountRequirement {
    /// Whether `found` artifacts or bindings meet the requirement.
    pub fn allows(&self, found: usize) -> bool {
        match *self {
            Self::Exactly(count) => found == count,
            Self::AtLeast(count) => found >= count,
        }
    }
}

impl fmt::Display for CountRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exactly(count) => write!(f, "exactly {count}"),
            Self::AtLeast(count) => write!(f, "at least {count}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageRule {
    pub action: CoverageAction,
    pub product: String,
    pub group_by: Vec<String>,
    pub count: CountRequirement,
    /// Entity values that must each be present in every group, such as
    /// `run=1,2`. Each listed dimension is checked on its own.
    pub values: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoverageAction {
    #[default]
    Require,
    Skip,
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
            count,
            values: BTreeMap::new(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub id: usize,
    pub operation: String,
    /// The artifacts bound to each input port, in port order. A many port
    /// holds its collection in order; every other port holds one artifact.
    pub inputs: Vec<Vec<ArtifactInstance>>,
    /// One artifact per output port, in port order.
    pub outputs: Vec<ArtifactInstance>,
    pub dependencies: Vec<usize>,
    /// The stage of the step that made this job, if any.
    pub stage: Option<String>,
}

impl Job {
    /// The first output artifact.
    ///
    /// # Panics
    ///
    /// If the job has no outputs. A resolved job always has one, since
    /// every operation declares at least one output.
    pub fn output(&self) -> &ArtifactInstance {
        &self.outputs[0]
    }

    /// Every input artifact, in port order.
    pub fn input_artifacts(&self) -> impl Iterator<Item = &ArtifactInstance> {
        self.inputs.iter().flatten()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedDag {
    pub jobs: Vec<Job>,
    /// Declaration order is retained for readable dry-run output.
    pub product_dimensions: BTreeMap<String, Vec<String>>,
    /// The file of each source whose inventory record gave one. Other
    /// artifacts take the path their product's rule gives them.
    pub source_paths: BTreeMap<ArtifactKey, String>,
}

impl ResolvedDag {
    /// Only the jobs of `stage` and the stages nested in it. Their inputs from
    /// other stages are taken as files that already exist, so dependencies on those jobs are dropped;
    /// every job keeps its number.
    #[must_use]
    pub fn only_stage(&self, stage: &str) -> Self {
        let jobs: Vec<_> = self
            .jobs
            .iter()
            .filter(|job| {
                job.stage
                    .as_deref()
                    .is_some_and(|name| stage_within(name, stage))
            })
            .cloned()
            .collect();
        let kept: BTreeSet<_> = jobs.iter().map(|job| job.id).collect();
        Self {
            jobs: jobs
                .into_iter()
                .map(|mut job| {
                    job.dependencies.retain(|id| kept.contains(id));
                    job
                })
                .collect(),
            product_dimensions: self.product_dimensions.clone(),
            source_paths: self.source_paths.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Gap {
    Unmatched(ResolveError),
    /// An input is the output of an incomplete job, or a source held back by
    /// a coverage gap.
    Blocked {
        port: String,
        artifact: ArtifactInstance,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncompleteJob {
    pub operation: String,
    pub stage: Option<String>,
    pub outputs: Vec<ArtifactInstance>,
    pub gaps: Vec<Gap>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageGap {
    pub error: ResolveError,
    pub sources: Vec<ArtifactInstance>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArtifactReport {
    pub sources: Vec<ArtifactInstance>,
    pub dag: ResolvedDag,
    pub incomplete: Vec<IncompleteJob>,
    pub coverage: Vec<CoverageGap>,
}

fn owned_strings(values: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect()
}
