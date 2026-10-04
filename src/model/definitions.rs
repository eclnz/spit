//! What a pipeline declares: products, operations and their ports, the
//! steps that call them, commands, sidecars and stages.

use std::collections::BTreeMap;
use std::fmt;

use crate::command::CommandTemplate;
use crate::paths::PathTemplate;
use crate::span::Place;

use super::{owned_strings, ArtifactInstance, ArtifactType};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductDef {
    pub name: String,
    pub artifact_type: ArtifactType,
    pub dimensions: Vec<String>,
    /// The extension a source declares its files have, as in
    /// `source events : Events .tsv [sub]`; `None` for an output, whose
    /// operation says.
    pub extension: Option<String>,
    /// Whether the source's artifacts are folders, as in
    /// `source dicom : Dicom / [sub]`; `false` for an output, whose
    /// operation says.
    pub folder: bool,
    /// The checks a source's artifacts must pass before a job reads them, as
    /// in `source t1w : Image [sub] @ check(ndim(3))`; none for an output.
    pub checks: Vec<CheckUse>,
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
            extension: None,
            folder: false,
            checks: Vec::new(),
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
    /// The checks each artifact the port reads must pass before the job
    /// runs, as in `dwi: DWI @ check(ndim(4))`.
    pub checks: Vec<CheckUse>,
}

impl InputPort {
    pub fn one(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::One,
            checks: Vec::new(),
        }
    }

    pub fn many(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::Many,
            checks: Vec::new(),
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
    /// Whether the tool writes a folder here rather than a file, as in
    /// `-> (subject: FsSubject /)`.
    pub folder: bool,
    /// For a file the tool writes beside another output without being told
    /// where: that output, and what this one's name ends with in place of
    /// its extension.
    pub beside: Option<Beside>,
    /// The checks the artifact must pass after the command writes it, as in
    /// `-> Image @ check(nonempty)`.
    pub checks: Vec<CheckUse>,
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
            folder: false,
            beside: None,
            checks: Vec::new(),
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
    /// The steps that carry the operation out, for one written with a body
    /// in place of a command; empty for one a command carries out. A call
    /// to it becomes these steps, each with its own jobs.
    pub steps: Vec<BodyStep>,
    /// The file an imported operation is declared in, relative to the
    /// pipeline's folder, with `/` between folders; `None` for one the
    /// pipeline declares.
    pub file: Option<String>,
}

/// A step in an operation's body, as written: the call it makes over the
/// operation's ports and the body's own products, and what it says of each
/// product it makes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BodyStep {
    pub invocation: Invocation,
    pub outputs: Vec<StepOutput>,
    /// Where the call is written, in the file that declares the operation.
    pub(crate) place: Place,
}

/// A product a step makes, with the type and dimensions the step writes for
/// it, if any; lowering infers the rest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepOutput {
    pub name: String,
    pub artifact_type: Option<ArtifactType>,
    pub dimensions: Option<Vec<String>>,
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
            steps: Vec::new(),
            file: None,
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
    /// For a step a call to an operation with a body made, that call and
    /// the step of the body it is; `None` for a step written in the
    /// pipeline.
    pub origin: Option<StepOrigin>,
    /// Checks the step runs beyond its operation's: those an operation with
    /// a body attaches to the inputs and outputs the step reads and makes.
    pub checks: Vec<(Port, CheckUse)>,
}

/// One of a step's ports, by its position among the operation's inputs or
/// outputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Port {
    Input(usize),
    Output(usize),
}

/// Where a step a call made comes from: the call, and where its step is
/// written in the body of the operation called.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepOrigin {
    pub call: CallId,
    pub(crate) step: Place,
}

/// A call's place in [`Pipeline::calls`](super::Pipeline::calls).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CallId(u32);

impl CallId {
    /// The call at `index` in `Pipeline::calls`.
    pub(crate) fn at(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 calls in a pipeline"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A call to an operation with a body, which lowering replaced by the
/// body's steps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Call {
    /// The operation called, as the caller names it.
    pub operation: String,
    /// The name its products are filed under: the call's first output, so
    /// that a product `p` of the body is `instance::p`.
    pub instance: String,
    /// The call whose body holds this one, for a call in a body.
    pub parent: Option<CallId>,
    /// Where the call is written: in the pipeline, or in the body of the
    /// parent's operation.
    pub(crate) place: Place,
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
            origin: None,
            checks: Vec::new(),
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

/// A test of one artifact, declared once as `check name(params): command`
/// and attached where it applies with `@ check(name(arguments))`. Its
/// command reads `{@path}`, the artifact, and each `{param}`, the text the
/// attachment gives; a nonzero exit fails the job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckDef {
    pub name: String,
    pub parameters: Vec<String>,
    pub template: CommandTemplate,
}

/// A check attached to a port or a source, with the text it gives each of
/// the check's parameters, as in `ndim(3)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckUse {
    pub check: String,
    pub arguments: Vec<String>,
}

/// Reads as written: `nonempty`, or `ndim(3)` with arguments.
impl fmt::Display for CheckUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.check)?;
        if !self.arguments.is_empty() {
            write!(f, "({})", self.arguments.join(", "))?;
        }
        Ok(())
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
    /// The stem the block's `path:` line gives, if it has one; otherwise a
    /// recipe gives it, as `path name: stem`.
    pub stem: Option<PathTemplate>,
}

impl SidecarGroup {
    /// The path rule each member takes from `stem`: the stem and the
    /// member's extension.
    pub fn member_paths<'a>(
        &'a self,
        stem: &'a PathTemplate,
    ) -> impl Iterator<Item = (&'a str, PathTemplate)> + 'a {
        self.members
            .iter()
            .map(move |(member, extension)| (member.as_str(), stem.with_extension(extension)))
    }
}

/// Where the extension a product's file must have is declared.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ExtensionSource {
    /// On the output of the named operation.
    Operation(String),
    /// On the named source's declaration.
    Source(String),
    /// By the named stage's `ext:` line.
    Stage(String),
    /// By the pipeline's `ext:` line.
    Default,
}

impl fmt::Display for ExtensionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation(operation) => write!(f, "operation `{operation}`"),
            Self::Source(source) => write!(f, "source `{source}`"),
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
