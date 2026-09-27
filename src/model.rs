use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use crate::error::ResolveError;
use crate::types::TypeExpr;

pub type ArtifactType = TypeExpr;

#[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct EntityBinding(pub BTreeMap<String, String>);

impl EntityBinding {
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
pub fn natural_cmp(left: &str, right: &str) -> Ordering {
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
pub const DEFAULT_OUTPUT: &str = "output";

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

    pub fn product_name(&self) -> &str {
        &self.product
    }

    /// Whether any `@` selector is present.
    pub fn has_selectors(&self) -> bool {
        self.vary.is_some() || !self.pinned.is_empty() || self.same.is_some()
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
        }
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
    pub template: String,
    pub role: CommandRole,
}

impl CommandDef {
    pub fn new(operation: impl Into<String>, template: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            template: template.into(),
            role: CommandRole::Run,
        }
    }

    pub fn verify(operation: impl Into<String>, template: impl Into<String>) -> Self {
        Self {
            role: CommandRole::Verify,
            ..Self::new(operation, template)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    pub products: Vec<ProductDef>,
    pub operations: Vec<OperationDef>,
    pub invocations: Vec<Invocation>,
    pub constraints: Vec<CoverageRule>,
    pub commands: Vec<CommandDef>,
    pub path_template: Option<String>,
    pub product_paths: BTreeMap<String, String>,
}

/// A source record identifies a logical artifact without binding it to a path.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceRecord {
    pub product: String,
    pub entities: EntityBinding,
}

impl SourceRecord {
    pub fn new(product: impl Into<String>, entities: EntityBinding) -> Self {
        Self {
            product: product.into(),
            entities,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CountRequirement {
    Exactly(usize),
    AtLeast(usize),
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
    pub product: String,
    pub group_by: Vec<String>,
    pub count: CountRequirement,
    /// Entity values that must each be present in every group, such as
    /// `run=1,2`. Each listed dimension is checked on its own.
    pub values: BTreeMap<String, Vec<String>>,
}

impl CoverageRule {
    pub fn new(
        product: impl Into<String>,
        group_by: impl IntoIterator<Item = impl AsRef<str>>,
        count: CountRequirement,
    ) -> Self {
        Self {
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
}

impl Job {
    /// The first output artifact.
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
}

/// Why a job cannot run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Gap {
    /// The inventory does not supply what an input needs:
    /// [`ResolveError::MissingInput`], [`ResolveError::AmbiguousInput`], or
    /// [`ResolveError::CollectionTooSmall`].
    Unmatched(ResolveError),
    /// An input artifact is itself incomplete: the output of a job that cannot
    /// run, or a source held back by a coverage rule.
    Blocked {
        port: String,
        artifact: ArtifactInstance,
    },
}

/// A job, and the artifacts it would produce, that cannot run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncompleteJob {
    pub operation: String,
    pub outputs: Vec<ArtifactInstance>,
    pub gaps: Vec<Gap>,
}

/// A group that fails a coverage rule, and the sources it holds back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageGap {
    /// [`ResolveError::CoverageViolation`] or
    /// [`ResolveError::MissingRequiredValue`].
    pub error: ResolveError,
    pub sources: Vec<ArtifactInstance>,
}

/// Every concrete artifact a pipeline yields from an inventory: the jobs
/// that can run, and those that cannot with the reasons why.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArtifactReport {
    /// Every source artifact, in product declaration order.
    pub sources: Vec<ArtifactInstance>,
    /// The jobs that can run once every incomplete one is left out.
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
