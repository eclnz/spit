use std::collections::BTreeMap;
use std::fmt;

use crate::span::Place;
use crate::template::CommandTemplate;
use crate::template::{EntitiesFormat, PathTemplate};

use super::{
    owned_strings, ArtifactInstance, ArtifactType, Beside, CheckUse, DefaultChecks, DefaultReviews,
    ToolList, ToolSet,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductDef {
    pub name: String,
    pub artifact_type: ArtifactType,
    pub dimensions: Vec<String>,
    pub extension: Option<String>,
    pub folder: bool,
    pub beside: Option<Beside>,
    pub checks: Vec<CheckUse>,
    pub exempt_checks: Vec<CheckUse>,
    pub reviews: Vec<String>,
    pub here: Option<String>,
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
            beside: None,
            checks: Vec::new(),
            exempt_checks: Vec::new(),
            reviews: Vec::new(),
            here: None,
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
    pub checks: Vec<CheckUse>,
    pub reviews: Vec<String>,
}

impl InputPort {
    pub fn one(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::One,
            checks: Vec::new(),
            reviews: Vec::new(),
        }
    }

    pub fn many(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            cardinality: Cardinality::Many,
            checks: Vec::new(),
            reviews: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShapeRule {
    Preserve,
    Aggregate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPort {
    pub name: String,
    pub artifact_type: ArtifactType,
    pub extension: Option<String>,
    pub naming: String,
    pub ending: Option<String>,
    pub folder: bool,
    pub beside: Option<Beside>,
    pub seed: Option<String>,
    pub view: bool,
    pub checks: Vec<CheckUse>,
    pub exempt: Vec<CheckUse>,
    pub reviews: Vec<String>,
    pub exempt_reviews: Vec<String>,
}

impl OutputPort {
    pub fn new(name: impl Into<String>, artifact_type: ArtifactType) -> Self {
        Self {
            name: name.into(),
            artifact_type,
            extension: None,
            naming: String::new(),
            ending: None,
            folder: false,
            beside: None,
            seed: None,
            view: false,
            checks: Vec::new(),
            exempt: Vec::new(),
            reviews: Vec::new(),
            exempt_reviews: Vec::new(),
        }
    }

    #[must_use]
    pub fn beside(mut self, beside: Beside) -> Self {
        self.extension = beside.extension().map(str::to_owned);
        self.ending = self.extension.clone();
        self.beside = Some(beside);
        self
    }

    #[must_use]
    pub fn with_naming(mut self, naming: impl Into<String>) -> Self {
        self.naming = naming.into();
        self.ending = self
            .extension
            .as_ref()
            .map(|extension| format!("{}{extension}", self.naming));
        self
    }

    #[must_use]
    pub fn with_extension(mut self, extension: impl Into<String>) -> Self {
        let extension = extension.into();
        self.ending = Some(format!("{}{extension}", self.naming));
        self.extension = Some(extension);
        self
    }
}

pub(crate) const DEFAULT_OUTPUT: &str = "output";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDef {
    pub name: String,
    pub inputs: Vec<InputPort>,
    pub outputs: Vec<OutputPort>,
    pub shape_rule: ShapeRule,
    pub minimum_collection: Option<usize>,
    pub steps: Vec<BodyStep>,
    pub file: Option<String>,
    pub tools: ToolSet,
    pub anchored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BodyStep {
    pub invocation: Invocation,
    pub outputs: Vec<StepOutput>,
    pub(crate) place: Place,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepOutput {
    pub name: String,
    pub artifact_type: Option<ArtifactType>,
    pub dimensions: Option<Vec<String>>,
}

impl OperationDef {
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
            tools: ToolSet::default(),
            anchored: false,
        }
    }

    #[must_use]
    pub fn at_least(mut self, minimum: usize) -> Self {
        self.minimum_collection = Some(minimum);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactSelection {
    First(String),
    Last(String),
}

impl ArtifactSelection {
    pub fn dimension(&self) -> &str {
        match self {
            Self::First(dimension) | Self::Last(dimension) => dimension,
        }
    }

    pub fn keyword(&self) -> &'static str {
        match self {
            Self::First(_) => "first",
            Self::Last(_) => "last",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputBinding {
    pub product: String,
    pub vary: Vec<String>,
    pub pinned: BTreeMap<String, String>,
    pub selection: Option<ArtifactSelection>,
    pub same: Option<Vec<String>>,
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

    pub fn has_selectors(&self) -> bool {
        !self.vary.is_empty()
            || !self.pinned.is_empty()
            || self.selection.is_some()
            || self.same.is_some()
            || !self.each.is_empty()
    }

    pub fn free_dimensions(&self, dimensions: &[String]) -> Vec<String> {
        let selected = self.selection.as_ref().map(ArtifactSelection::dimension);
        dimensions
            .iter()
            .filter(|dimension| {
                !self.pinned.contains_key(*dimension) && selected != Some(dimension.as_str())
            })
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub operation: String,
    pub inputs: Vec<InputBinding>,
    pub outputs: Vec<String>,
    pub stage: Option<String>,
    pub origin: Option<StepOrigin>,
    pub line: Option<usize>,
    pub checks: Vec<(Port, CheckUse)>,
    pub reviews: Vec<ReviewUse>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewUse {
    pub port: Port,
    pub name: Option<String>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Port {
    Input(usize),
    Output(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepOrigin {
    pub call: CallId,
    pub(crate) step: Place,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CallId(u32);

impl CallId {
    pub(crate) fn at(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 calls in a pipeline"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Call {
    pub operation: String,
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub parent: Option<CallId>,
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
            line: None,
            checks: Vec::new(),
            reviews: Vec::new(),
        }
    }

    #[must_use]
    pub fn in_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    pub fn output_product(&self) -> &str {
        self.outputs.first().map_or("", String::as_str)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandRole {
    Run,
    Verify,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDef {
    pub operation: String,
    pub lines: Vec<CommandTemplate>,
    pub role: CommandRole,
}

impl CommandDef {
    pub fn new(operation: impl Into<String>, template: CommandTemplate) -> Self {
        Self {
            operation: operation.into(),
            lines: vec![template],
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Resources {
    pub cores: u32,
    pub memory: Option<u64>,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            cores: 1,
            memory: None,
        }
    }
}

impl fmt::Display for Resources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cores={}", self.cores)?;
        if let Some(bytes) = self.memory {
            f.write_str(", memory=")?;
            write_memory(f, bytes)?;
        }
        Ok(())
    }
}

pub(crate) const MEMORY_UNITS: [(char, u32); 4] = [('K', 10), ('M', 20), ('G', 30), ('T', 40)];

fn write_memory(f: &mut fmt::Formatter<'_>, bytes: u64) -> fmt::Result {
    let (unit, shift) = MEMORY_UNITS
        .iter()
        .rev()
        .find(|&&(_, shift)| bytes.trailing_zeros() >= shift)
        .copied()
        .unwrap_or(('K', 10));
    write!(f, "{}{unit}", bytes >> shift)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcesDef {
    pub operation: String,
    pub resources: Resources,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SidecarGroup {
    pub name: String,
    pub dimensions: Vec<String>,
    pub members: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ExtensionSource {
    Operation(String),
    Source(String),
    Stage(String),
    Default,
}

impl fmt::Display for ExtensionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation(operation) => write!(f, "operation `{operation}`"),
            Self::Source(source) => write!(f, "source `{source}`"),
            Self::Stage(stage) => write!(f, "stage `{stage}`'s `default ext:`"),
            Self::Default => f.write_str("`default ext:`"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageDef {
    pub name: String,
    pub path_template: Option<PathTemplate>,
    pub extension: Option<String>,
    pub checks: DefaultChecks,
    pub reviews: DefaultReviews,
    pub uses: Option<ToolList>,
    pub entities_format: Option<EntitiesFormat>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPattern {
    pub dimensions: Vec<String>,
    pub template: PathTemplate,
}

impl StageDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path_template: None,
            extension: None,
            checks: DefaultChecks::default(),
            reviews: DefaultReviews::default(),
            uses: None,
            entities_format: None,
        }
    }
}

impl ProductDef {
    pub fn sort_family(&self, family: &mut [ArtifactInstance]) {
        family.sort_by(|left, right| left.entities.cmp_in(&right.entities, &self.dimensions));
    }
}

pub(crate) fn stage_and_parents(stage: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(stage), |name| {
        name.rsplit_once('/').map(|(parent, _)| parent)
    })
}

pub fn stage_within(stage: &str, outer: &str) -> bool {
    stage
        .strip_prefix(outer)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}
