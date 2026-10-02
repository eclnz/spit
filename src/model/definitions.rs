//! What a pipeline declares: products, operations and their ports, the
//! steps that call them, commands, sidecars and stages.

use std::collections::BTreeMap;
use std::fmt;

use crate::command::CommandTemplate;
use crate::paths::PathTemplate;

use super::{owned_strings, ArtifactInstance, ArtifactType};

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
    /// The extension the operation's tool gives this output's file, such as
    /// `.nii.gz`, when the operation declares one.
    pub extension: Option<String>,
    /// For a file the tool writes beside another output without being told
    /// where: that output, and what this one's name ends with in place of
    /// its extension.
    pub beside: Option<Beside>,
}

/// An output written beside the named port's file, its name that file's
/// without its extension, then `suffix`: `.json`, or `_mask.nii.gz`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Beside {
    pub port: String,
    pub suffix: String,
}

impl OutputPort {
    pub fn new(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            extension: None,
            beside: None,
        }
    }

    /// This output written beside `port`, its name ending with `suffix`. Its
    /// extension is the suffix from its first `.`, if it has one.
    #[must_use]
    pub fn beside(mut self, port: impl Into<String>, suffix: impl Into<String>) -> Self {
        let suffix = suffix.into();
        self.extension = suffix.find('.').map(|dot| suffix[dot..].to_owned());
        self.beside = Some(Beside {
            port: port.into(),
            suffix,
        });
        self
    }

    #[must_use]
    pub fn with_extension(mut self, extension: impl Into<String>) -> Self {
        self.extension = Some(extension.into());
        self
    }
}

/// The port name of an operation's only, unnamed output.
pub(crate) const DEFAULT_OUTPUT: &str = "output";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDef {
    pub name: String,
    pub inputs: Vec<InputPort>,
    /// Every artifact one job writes, in declaration order.
    pub outputs: Vec<OutputPort>,
    pub shape_rule: ShapeRule,
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
            minimum_collection: None,
        }
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
    /// `@ vary(dimensions)`: collect artifacts that differ in these dimensions.
    pub vary: Vec<String>,
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
            vary: vec![dimension.into()],
            ..Self::product(product)
        }
    }

    pub fn varying(
        product: impl Into<String>,
        dimensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            vary: dimensions.into_iter().map(Into::into).collect(),
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
        !self.vary.is_empty()
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

/// Sources declared together in a `sidecars` block: they share dimensions
/// and a path stem, and differ by extension, as a photo and its GPS track.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidecarGroup {
    pub name: String,
    pub dimensions: Vec<String>,
    /// Each member source with its extension, in declaration order.
    pub members: Vec<(String, String)>,
}

/// Where the extension a product's file must have is declared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionSource {
    /// On the output of the named operation.
    Operation(String),
    /// By the named stage's `ext:` line.
    Stage(String),
    /// By the pipeline's `ext:` line.
    Default,
}

impl fmt::Display for ExtensionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation(operation) => write!(f, "operation `{operation}`"),
            Self::Stage(stage) => write!(f, "stage `{stage}`'s `ext:`"),
            Self::Default => f.write_str("`ext:`"),
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
    /// The `ext:` default for the same products, in place of the
    /// pipeline's.
    pub extension: Option<String>,
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
            extension: None,
        }
    }
}

impl ProductDef {
    /// Order this product's artifacts by its declared dimensions, reading
    /// numbers as numbers.
    pub fn sort_family(&self, family: &mut [ArtifactInstance]) {
        family.sort_by(|left, right| left.entities.cmp_in(&right.entities, &self.dimensions));
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
