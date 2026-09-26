use std::collections::BTreeMap;
use std::fmt;

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
    /// The first input drives one output per artifact and preserves its dimensions.
    Preserve,
    /// One many-valued input groups by the dimension named in its `vary` binding.
    Aggregate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDef {
    pub name: String,
    pub inputs: Vec<InputPort>,
    pub output_type: ArtifactType,
    pub shape_rule: ShapeRule,
    /// An optional declared dimension consumed by an aggregate operation.
    pub aggregated_dimension: Option<String>,
}

impl OperationDef {
    pub fn new(
        name: impl Into<String>,
        inputs: Vec<InputPort>,
        output_type: ArtifactType,
        shape_rule: ShapeRule,
    ) -> Self {
        Self {
            name: name.into(),
            inputs,
            output_type,
            shape_rule,
            aggregated_dimension: None,
        }
    }

    #[must_use]
    pub fn aggregating(mut self, dimension: impl Into<String>) -> Self {
        self.aggregated_dimension = Some(dimension.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputBinding {
    Product(String),
    Vary { product: String, dimension: String },
}

impl InputBinding {
    pub fn product(name: impl Into<String>) -> Self {
        Self::Product(name.into())
    }

    pub fn vary(product: impl Into<String>, dimension: impl Into<String>) -> Self {
        Self::Vary {
            product: product.into(),
            dimension: dimension.into(),
        }
    }

    pub fn product_name(&self) -> &str {
        match self {
            Self::Product(name) | Self::Vary { product: name, .. } => name,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub operation: String,
    /// Bindings correspond to operation ports in declaration order.
    pub inputs: Vec<InputBinding>,
    pub output_product: String,
}

impl Invocation {
    pub fn new(
        operation: impl Into<String>,
        inputs: Vec<InputBinding>,
        output_product: impl Into<String>,
    ) -> Self {
        Self {
            operation: operation.into(),
            inputs,
            output_product: output_product.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDef {
    pub operation: String,
    pub template: String,
}

impl CommandDef {
    pub fn new(operation: impl Into<String>, template: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            template: template.into(),
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
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub id: usize,
    pub operation: String,
    /// One port may contribute several artifacts for aggregation.
    pub inputs: Vec<ArtifactInstance>,
    pub output: ArtifactInstance,
    pub dependencies: Vec<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedDag {
    pub jobs: Vec<Job>,
    /// Declaration order is retained for readable dry-run output.
    pub product_dimensions: BTreeMap<String, Vec<String>>,
}

fn owned_strings(values: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect()
}
