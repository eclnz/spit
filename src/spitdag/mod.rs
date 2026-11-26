mod artifact_json;
mod reviews;
mod work;
mod write;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::Arc;

use crate::model::{
    identity, natural_cmp, ArtifactId, ArtifactInstance, Artifacts, EntityBinding, JobId, Removal,
    Resources, SourceFile, StepId, Tool, ToolIdentity, ToolSet, Under,
};
use crate::template::scalar_text;
use crate::types::TypeExpr;

pub use crate::check::When;

pub use self::reviews::{BoundReview, BoundTarget};
use self::write::{write_document, PIECE};

pub const SPITDAG_VERSION: usize = 19;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundDag {
    pub root: Option<String>,
    pub output_root: Option<String>,
    pub removed: Vec<Removal>,
    pub left_out: Vec<LeftOut>,
    pub reviews: Vec<BoundReview>,
    pub screen_records: BTreeMap<String, String>,
    pub jobs: Vec<BoundJob>,
    pub steps: Vec<BoundStep>,
    pub pipeline_files: Vec<SourceFile>,
    pub pipeline_folder: Option<String>,
    pub calls: Vec<BoundCall>,
    pub tools: Vec<Tool>,
    artifacts: Artifacts,
    pub(crate) views: BTreeMap<ArtifactId, Vec<ArtifactId>>,
    paths: Vec<String>,
    under: Vec<Under>,
    dimensions: Vec<Vec<String>>,
    folders: Vec<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeftOut {
    pub artifact: ArtifactInstance,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundStep {
    pub operation: String,
    pub stage: Option<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub seeds: Vec<(usize, usize)>,
    pub collection: Option<BoundCollection>,
    pub checks: Vec<StepCheck>,
    pub origin: Option<StepCall>,
    pub resources: Resources,
    pub tools: ToolSet,
    pub anchored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCollection {
    pub ports: Vec<usize>,
    pub varied: Vec<String>,
    pub minimum: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepCall {
    pub call: Option<usize>,
    pub file: Option<usize>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCall {
    pub operation: String,
    pub instance: String,
    pub parent: Option<usize>,
    pub file: Option<usize>,
    pub at_file: Option<usize>,
    pub at_line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepCheck {
    pub when: When,
    pub port: usize,
    pub written: String,
    pub kind: crate::model::CheckKind,
    pub product_scope: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCheck {
    pub check: usize,
    pub artifact: ArtifactId,
    pub command: Vec<Argument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJob {
    pub id: JobId,
    pub step: StepId,
    pub inputs: Vec<Vec<ArtifactId>>,
    pub outputs: Vec<ArtifactId>,
    pub depends_on: Vec<JobId>,
    pub missing: Vec<crate::model::MissingInput>,
    pub commands: Vec<BoundCommand>,
    pub verify: Vec<Vec<Argument>>,
    pub checks: Vec<BoundCheck>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCommand {
    pub arguments: Vec<Argument>,
    pub stdin: Option<ArtifactId>,
    pub stdout: Option<ArtifactId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundArtifact<'a> {
    pub id: ArtifactId,
    pub product: &'a str,
    pub artifact_type: &'a TypeExpr,
    pub path: &'a str,
    pub under: Under,
    pub folder: bool,
    entities: &'a EntityBinding,
    dimensions: &'a [String],
}

pub type Argument = Vec<ArgPart>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgPart {
    Text(String),
    Calculate(Arc<crate::template::Expression>),
    Path(ArtifactId),
    Dir(ArtifactId),
    Stem {
        artifact: ArtifactId,
        extension_len: usize,
    },
    Here {
        relative: String,
        text: String,
    },
    Scalar {
        artifact: ArtifactId,
        min: Option<Arc<str>>,
        max: Option<Arc<str>>,
    },
    Collection {
        port: usize,
        artifacts: Vec<ArtifactId>,
        separator: Option<Arc<str>>,
    },
    Member {
        port: usize,
        offset: usize,
        value: Box<ArgPart>,
    },
    JoinedValues {
        port: usize,
        values: Vec<String>,
        separator: Arc<str>,
    },
}

impl ArgPart {
    pub fn text<'a>(&'a self, dag: &'a BoundDag) -> &'a str {
        match self {
            Self::Collection { .. } | Self::JoinedValues { .. } => {
                unreachable!("a collection's paths are written and shown one by one")
            }
            Self::Member { value, .. } => value.text(dag),
            Self::Calculate(_) => "calculate",
            Self::Text(text) | Self::Here { text, .. } => text,
            Self::Path(artifact) | Self::Scalar { artifact, .. } => dag.path(*artifact),
            Self::Dir(artifact) => folder_of(dag.path(*artifact)),
            Self::Stem {
                artifact,
                extension_len,
            } => stem_of(dag.path(*artifact), *extension_len),
        }
    }
}

impl ArgPart {
    pub fn shown<'a>(&'a self, dag: &'a BoundDag) -> Cow<'a, str> {
        match self {
            Self::Calculate(expression) => {
                let mut text = String::from("calculate ");
                write::write_expression(&mut text, expression);
                Cow::Owned(text)
            }
            Self::Collection {
                artifacts,
                separator,
                ..
            } => {
                let paths: Vec<Cow<'_, str>> = artifacts
                    .iter()
                    .map(|&artifact| dag.shown_path(artifact))
                    .collect();
                Cow::Owned(match separator {
                    Some(separator) => paths.join(&**separator),
                    None => format!("@list({})", paths.join(", ")),
                })
            }
            Self::Member { value, .. } => value.shown(dag),
            Self::JoinedValues {
                values, separator, ..
            } => Cow::Owned(values.join(separator)),
            Self::Scalar { artifact, min, max } => Cow::Owned(scalar_text(
                &dag.shown_path(*artifact),
                min.as_deref(),
                max.as_deref(),
            )),
            Self::Path(artifact) => dag.shown_path(*artifact),
            Self::Dir(artifact) => match dag.shown_path(*artifact) {
                Cow::Borrowed(path) => Cow::Borrowed(folder_of(path)),
                Cow::Owned(path) => Cow::Owned(folder_of(&path).to_owned()),
            },
            _ => Cow::Borrowed(self.text(dag)),
        }
    }
}

fn relative_folder(from: &str, to: &str) -> Option<String> {
    if !from.starts_with('/') || !to.starts_with('/') {
        return None;
    }
    let (from, to) = (folder_parts(from), folder_parts(to));
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut relative = vec![".."; from.len() - shared];
    relative.extend(&to[shared..]);
    Some(if relative.is_empty() {
        ".".to_owned()
    } else {
        relative.join("/")
    })
}

fn folder_parts(path: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(folder, _)| folder)
}

fn stem_of(path: &str, extension_len: usize) -> &str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    name.len()
        .checked_sub(extension_len)
        .and_then(|end| name.get(..end))
        .unwrap_or(name)
}

impl<'a> BoundArtifact<'a> {
    pub fn entities(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        let entities = self.entities;
        self.dimensions
            .iter()
            .filter_map(|dimension| Some((dimension.as_str(), entities.get(dimension)?)))
    }

    pub fn identity(&self) -> String {
        identity(self.product, self.entities())
    }
}

impl BoundJob {
    pub fn input_artifacts(&self) -> impl Iterator<Item = ArtifactId> + '_ {
        self.inputs.iter().flatten().copied()
    }
}

impl BoundDag {
    pub(crate) fn new(
        artifacts: Artifacts,
        paths: Vec<String>,
        under: Vec<Under>,
        dimensions: Vec<Vec<String>>,
        folders: Vec<bool>,
        steps: Vec<BoundStep>,
        jobs: Vec<BoundJob>,
    ) -> Self {
        Self {
            root: None,
            output_root: None,
            removed: Vec::new(),
            left_out: Vec::new(),
            reviews: Vec::new(),
            screen_records: BTreeMap::new(),
            jobs,
            steps,
            pipeline_files: Vec::new(),
            pipeline_folder: None,
            calls: Vec::new(),
            tools: Vec::new(),
            artifacts,
            views: BTreeMap::new(),
            paths,
            under,
            dimensions,
            folders,
        }
    }

    pub fn step(&self, job: &BoundJob) -> &BoundStep {
        &self.steps[job.step.index()]
    }

    pub fn artifact(&self, id: ArtifactId) -> BoundArtifact<'_> {
        let artifact = self.artifacts.get(id);
        let product = self.artifacts.product_of(id) as usize;
        BoundArtifact {
            id,
            product: artifact.product,
            artifact_type: artifact.artifact_type,
            path: &self.paths[id.index()],
            under: self.under[id.index()],
            folder: self.folders[product],
            entities: artifact.entities,
            dimensions: &self.dimensions[product],
        }
    }

    pub fn path(&self, id: ArtifactId) -> &str {
        &self.paths[id.index()]
    }

    pub fn under(&self, id: ArtifactId) -> Under {
        self.under[id.index()]
    }

    pub fn shown_path(&self, id: ArtifactId) -> Cow<'_, str> {
        let path = self.path(id);
        if let (Under::Pipeline, Some(folder)) = (self.under(id), &self.pipeline_folder) {
            return Cow::Owned(format!("{folder}/{path}"));
        }
        match (self.under(id), &self.root, &self.output_root) {
            (Under::Root, Some(root), Some(output)) if root != output => {
                let folder = relative_folder(output, root).unwrap_or_else(|| root.clone());
                Cow::Owned(format!("{folder}/{path}"))
            }
            _ => Cow::Borrowed(path),
        }
    }

    fn produced(&self) -> Vec<bool> {
        let mut produced = vec![false; self.paths.len()];
        for &output in self.jobs.iter().flat_map(|job| &job.outputs) {
            produced[output.index()] = true;
        }
        produced
    }

    pub fn external_inputs(&self) -> Vec<BoundArtifact<'_>> {
        let mut seen = self.produced();
        let mut external = Vec::new();
        let evidence = self
            .reviews
            .iter()
            .flat_map(|review| &review.targets)
            .flat_map(|target| target.evidence.iter().copied());
        for input in self
            .jobs
            .iter()
            .flat_map(BoundJob::input_artifacts)
            .chain(evidence)
        {
            if !seen[input.index()] && !self.views.contains_key(&input) {
                seen[input.index()] = true;
                external.push(input);
            }
        }
        external.sort_unstable_by(|&left, &right| natural_cmp(self.path(left), self.path(right)));
        external.into_iter().map(|id| self.artifact(id)).collect()
    }

    pub fn targets(&self) -> Vec<BoundArtifact<'_>> {
        let mut read = vec![false; self.paths.len()];
        for input in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            read[input.index()] = true;
        }
        for member in self.views.values().flatten() {
            read[member.index()] = true;
        }
        self.jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .chain(self.views.keys())
            .filter(|output| !read[output.index()])
            .map(|&output| self.artifact(output))
            .collect()
    }

    pub fn executables(&self) -> BTreeSet<String> {
        self.jobs
            .iter()
            .flat_map(|job| {
                let checks = job.checks.iter().map(|check| &check.command);
                let commands = job.commands.iter().map(|command| &command.arguments);
                commands.chain(&job.verify).chain(checks)
            })
            .filter_map(|command| {
                command
                    .first()?
                    .iter()
                    .map(|part| match part {
                        ArgPart::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect()
            })
            .chain(self.tools.iter().filter_map(|tool| match &tool.identity {
                ToolIdentity::Probe(command) => command.first().cloned(),
                ToolIdentity::Declared(_) => None,
            }))
            .collect()
    }

    pub fn step_tools<'a>(&'a self, step: &'a BoundStep) -> impl Iterator<Item = &'a str> {
        self.tools
            .iter()
            .filter(|tool| step.tools.includes(&tool.name))
            .map(|tool| tool.name.as_str())
    }

    pub fn dependents(&self) -> BTreeMap<JobId, Vec<JobId>> {
        let mut dependents: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for job in &self.jobs {
            for &dependency in &job.depends_on {
                dependents.entry(dependency).or_default().push(job.id);
            }
        }
        dependents
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        let kept: io::Result<()> = write_document(self, &mut out, |_| Ok(()));
        debug_assert!(kept.is_ok());
        out
    }

    pub fn write_json(&self, writer: &mut impl io::Write) -> io::Result<()> {
        let mut out = String::with_capacity(2 * PIECE);
        write_document(self, &mut out, |out| {
            writer.write_all(out.as_bytes())?;
            out.clear();
            Ok(())
        })?;
        writer.write_all(out.as_bytes())?;
        writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::relative_folder;

    #[test]
    fn a_folder_is_reached_from_another_as_written() {
        let reach = |from, to| relative_folder(from, to);
        assert_eq!(
            reach("/study/results", "/study/data").as_deref(),
            Some("../data")
        );
        assert_eq!(
            reach("/study/out/../results", "/study/data/").as_deref(),
            Some("../data")
        );
        assert_eq!(reach("/study", "/study/data").as_deref(), Some("data"));
        assert_eq!(reach("/study/a/b", "/study").as_deref(), Some("../.."));
        assert_eq!(reach("/study", "/study/.").as_deref(), Some("."));
        assert_eq!(reach("C:\\results", "C:\\data"), None);
    }
}
