//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

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

pub use self::reviews::{BoundReview, BoundTarget, Rejection};
use self::write::{write_document, PIECE};

/// The schema version a `.spitdag` is written with.
pub const SPITDAG_VERSION: usize = 15;

/// A resolved DAG with its paths bound and its commands expanded. Its
/// artifacts are the resolved DAG's, each kept once with its path; jobs,
/// and the paths in their commands, refer to them by id.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundDag {
    /// The absolute dataset folder, which sources are under, when known.
    pub root: Option<String>,
    /// The absolute folder the other outputs are under, when known: the
    /// dataset root when no other is declared.
    pub output_root: Option<String>,
    /// What the input stage left out of the dataset, and why.
    pub removed: Vec<Removal>,
    /// Outputs whose jobs could not be planned, with the reasons from `artifacts`.
    pub left_out: Vec<LeftOut>,
    /// Each review the pipeline attaches, with what it withheld.
    pub reviews: Vec<BoundReview>,
    /// Each job after the jobs it depends on.
    pub jobs: Vec<BoundJob>,
    /// Each step of the resolved DAG, with its ports named, which jobs
    /// refer to by [`StepId`].
    pub steps: Vec<BoundStep>,
    /// The files the pipeline was read from, with their blob ids.
    pub pipeline_files: Vec<SourceFile>,
    /// The pipeline's folder, as an absolute path, which an [`ArgPart::Here`]
    /// is under; `None` when the pipeline was not read from a file.
    pub pipeline_folder: Option<String>,
    /// Each call to an operation with a body, which steps name.
    pub calls: Vec<BoundCall>,
    /// What each tool is, from the recipe's `.spitenv`; empty without one,
    /// when jobs depend on no tool.
    pub tools: Vec<Tool>,
    artifacts: Artifacts,
    /// Each artifact's file, relative to its root, by id; empty for an
    /// artifact no job uses.
    paths: Vec<String>,
    /// The root each artifact's file is under, by id.
    under: Vec<Under>,
    /// Each product's dimensions in declared order, by product number.
    dimensions: Vec<Vec<String>>,
    /// Whether each product's artifacts are folders, by product number.
    folders: Vec<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeftOut {
    pub artifact: ArtifactInstance,
    pub reasons: Vec<String>,
}

/// What every job of one step shares, kept once in the bound DAG rather
/// than in each job: the operation, the stage, and the names of its ports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundStep {
    pub operation: String,
    /// The stage whose block holds the step, as `outer/inner`, if any.
    pub stage: Option<String>,
    /// The name of each input port, in port order.
    pub inputs: Vec<String>,
    /// The name of each output port, in port order.
    pub outputs: Vec<String>,
    /// The checks its jobs run, which their [`BoundCheck`]s refer to by
    /// index.
    pub checks: Vec<StepCheck>,
    /// Where the step is written, when it was read from a file.
    pub origin: Option<StepCall>,
    /// What each of its jobs needs to run.
    pub resources: Resources,
    /// The tools each of its jobs depends on.
    pub tools: ToolSet,
}

/// Where a step is written: the line and its file, and for a step a call
/// made, the call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StepCall {
    /// The call's position in [`BoundDag::calls`]; `None` for a step
    /// written in the pipeline.
    pub call: Option<usize>,
    /// The file the line is in, in [`BoundDag::pipeline_files`]: the pipeline's
    /// for a step written there, the one declaring the call's operation for a
    /// step the call made.
    pub file: Option<usize>,
    pub line: usize,
}

/// A call to an operation with a body, which the DAG holds as its steps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCall {
    pub operation: String,
    /// The name the call's own products are filed under.
    pub instance: String,
    /// The call whose body holds this one, by position.
    pub parent: Option<usize>,
    /// The file declaring the operation called, in [`BoundDag::pipeline_files`].
    pub file: Option<usize>,
    /// Where the call is written: a file in [`BoundDag::pipeline_files`], and a line.
    pub at_file: Option<usize>,
    pub at_line: usize,
}

/// A check every job of a step runs on the artifacts of one port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepCheck {
    pub when: When,
    /// The input port, or for [`When::After`] the output port, by index.
    pub port: usize,
    /// The check as written, as in `ndim(4)`.
    pub written: String,
}

/// A check one job runs on one artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCheck {
    /// The check, in its step's [`BoundStep::checks`].
    pub check: usize,
    pub artifact: ArtifactId,
    pub command: Vec<Argument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJob {
    pub id: JobId,
    /// The step that made this job, in [`BoundDag::steps`], which names its
    /// ports.
    pub step: StepId,
    /// The artifacts of each input port, in its step's port order.
    pub inputs: Vec<Vec<ArtifactId>>,
    /// The artifact of each output port, in its step's port order.
    pub outputs: Vec<ArtifactId>,
    pub depends_on: Vec<JobId>,
    /// The commands that make the outputs, run in order until one fails;
    /// empty when the operation has none.
    pub commands: Vec<BoundCommand>,
    /// Commands that check the inputs before the job runs.
    pub verify: Vec<Vec<Argument>>,
    /// The checks of single artifacts: those on inputs, then those on
    /// outputs.
    pub checks: Vec<BoundCheck>,
}

/// One command of a job, and the files its streams are connected to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCommand {
    pub arguments: Vec<Argument>,
    /// The input whose file the command reads on its stdin, if any.
    pub stdin: Option<ArtifactId>,
    /// The output whose file the command writes on its stdout, if any. It
    /// is never also among this command's arguments, and at most one of a
    /// job's commands has one.
    pub stdout: Option<ArtifactId>,
}

/// An artifact of a bound DAG, with its path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundArtifact<'a> {
    pub product: &'a str,
    pub artifact_type: &'a TypeExpr,
    /// The file or folder, relative to `under`.
    pub path: &'a str,
    /// The root `path` is relative to.
    pub under: Under,
    /// Whether the artifact is a folder rather than a file.
    pub folder: bool,
    entities: &'a EntityBinding,
    dimensions: &'a [String],
}

/// One command-line argument: literal text and artifact paths, joined.
pub type Argument = Vec<ArgPart>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgPart {
    Text(String),
    /// An artifact's file, relative to its root.
    Path(ArtifactId),
    /// The folder of an artifact's file, as `{image.dir}` gives it.
    Dir(ArtifactId),
    /// An artifact's file name without its extension, as `{image.stem}`
    /// gives it for a tool that adds the extension itself. `extension_len`
    /// is the byte length of the output port's extension, which the artifact's
    /// path ends with, so no job holds a copy of the extension's text.
    Stem {
        artifact: ArtifactId,
        extension_len: usize,
    },
    /// The folder of the file declaring the operation, as `{@here}` gives
    /// it. `relative` is under [`BoundDag::pipeline_folder`], `""` for the
    /// folder itself; `text` is the two joined, so that no job holds a
    /// second copy. Only `relative` is written, so a moved pipeline writes the
    /// same commands.
    Here {
        relative: String,
        text: String,
    },
    /// The number in an input's file, which a backend reads when the job
    /// starts, as `@scalar(port, min=N, max=N)` gives it. Each bound is the
    /// shortest decimal for its value.
    Scalar {
        artifact: ArtifactId,
        min: Option<Arc<str>>,
        max: Option<Arc<str>>,
    },
}

impl ArgPart {
    /// The text this part is in a command, given the DAG it belongs to. A
    /// scalar's is the path of the file it reads, since its number is known
    /// only when the job starts.
    pub fn text<'a>(&'a self, dag: &'a BoundDag) -> &'a str {
        match self {
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
    /// The text this part is in a command run from the output root: as
    /// [`ArgPart::text`], with a path under a separate dataset root made
    /// absolute, as [`BoundDag::shown_path`] gives it. A scalar is shown as
    /// a pipeline writes it, with its file's path for its port:
    /// `@scalar(tr/sub=01.txt, min=5)`.
    pub fn shown<'a>(&'a self, dag: &'a BoundDag) -> Cow<'a, str> {
        match self {
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

/// The folder `to` as reached from the folder `from`, both absolute and
/// `/`-separated, with `.` and `..` read as written rather than through
/// links: `../data` from `/study/results` to `/study/data`. `None` when
/// either does not start with `/`.
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

/// The names of the folders `path` goes through, with `.` and `..` read
/// as written.
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

/// The folder `path` is in, `.` for the root itself.
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(folder, _)| folder)
}

/// The file name of `path` without its last `extension_len` bytes, which are
/// the extension its output port declares.
fn stem_of(path: &str, extension_len: usize) -> &str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    name.len()
        .checked_sub(extension_len)
        .and_then(|end| name.get(..end))
        .unwrap_or(name)
}

impl<'a> BoundArtifact<'a> {
    /// Each dimension and value, in the product's declared order.
    pub fn entities(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        let entities = self.entities;
        self.dimensions
            .iter()
            .filter_map(|dimension| Some((dimension.as_str(), entities.get(dimension)?)))
    }

    /// Its product and bindings, as `image[sub=1,ses=2]`.
    pub fn identity(&self) -> String {
        identity(self.product, self.entities())
    }
}

impl BoundJob {
    /// Every input artifact, in port order.
    pub fn input_artifacts(&self) -> impl Iterator<Item = ArtifactId> + '_ {
        self.inputs.iter().flatten().copied()
    }
}

impl BoundDag {
    /// `jobs` over `artifacts`, where the artifact with each id has the path
    /// `paths` holds for it, under the root `under` holds, and each product,
    /// by number, has `dimensions` and is a folder where `folders` says so.
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
            jobs,
            steps,
            pipeline_files: Vec::new(),
            pipeline_folder: None,
            calls: Vec::new(),
            tools: Vec::new(),
            artifacts,
            paths,
            under,
            dimensions,
            folders,
        }
    }

    /// The step that made `job`, with its operation, stage and ports.
    pub fn step(&self, job: &BoundJob) -> &BoundStep {
        &self.steps[job.step.index()]
    }

    pub fn artifact(&self, id: ArtifactId) -> BoundArtifact<'_> {
        let artifact = self.artifacts.get(id);
        let product = self.artifacts.product_of(id) as usize;
        BoundArtifact {
            product: artifact.product,
            artifact_type: artifact.artifact_type,
            path: &self.paths[id.index()],
            under: self.under[id.index()],
            folder: self.folders[product],
            entities: artifact.entities,
            dimensions: &self.dimensions[product],
        }
    }

    /// The path of `id`, relative to its root.
    pub fn path(&self, id: ArtifactId) -> &str {
        &self.paths[id.index()]
    }

    /// The root the path of `id` is relative to.
    pub fn under(&self, id: ArtifactId) -> Under {
        self.under[id.index()]
    }

    /// The path of `id` as a command run from the output root names it:
    /// as written, unless it is under a dataset root apart from the output
    /// root, when it is reached from the output root, as `../data/in.txt`.
    pub fn shown_path(&self, id: ArtifactId) -> Cow<'_, str> {
        let path = self.path(id);
        match (self.under(id), &self.root, &self.output_root) {
            (Under::Root, Some(root), Some(output)) if root != output => {
                // A folder not written with `/`, as on Windows, is given whole.
                let folder = relative_folder(output, root).unwrap_or_else(|| root.clone());
                Cow::Owned(format!("{folder}/{path}"))
            }
            _ => Cow::Borrowed(path),
        }
    }

    /// Whether each artifact, by id, is an output of a job here.
    fn produced(&self) -> Vec<bool> {
        let mut produced = vec![false; self.paths.len()];
        for &output in self.jobs.iter().flat_map(|job| &job.outputs) {
            produced[output.index()] = true;
        }
        produced
    }

    /// The inputs no job here makes, each once, by path: the sources.
    pub fn external_inputs(&self) -> Vec<BoundArtifact<'_>> {
        let mut seen = self.produced();
        let mut external = Vec::new();
        for input in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            if !seen[input.index()] {
                seen[input.index()] = true;
                external.push(input);
            }
        }
        // In the order `many` inputs take, so `wave10` follows `wave2`. Every
        // artifact has its own path, so the order is total.
        external.sort_unstable_by(|&left, &right| natural_cmp(self.path(left), self.path(right)));
        external.into_iter().map(|id| self.artifact(id)).collect()
    }

    /// The outputs no job here reads: what a full run leaves behind, in job
    /// order.
    pub fn targets(&self) -> Vec<BoundArtifact<'_>> {
        let mut read = vec![false; self.paths.len()];
        for input in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            read[input.index()] = true;
        }
        self.jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .filter(|output| !read[output.index()])
            .map(|&output| self.artifact(output))
            .collect()
    }

    /// The programs the commands run, each once: every command's first
    /// argument that is plain text, and each tool's probe's, so a backend can
    /// look for them before it starts.
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

    /// The names of the tools in [`BoundDag::tools`] that each job of `step`
    /// depends on, in that order.
    pub fn step_tools<'a>(&'a self, step: &'a BoundStep) -> impl Iterator<Item = &'a str> {
        self.tools
            .iter()
            .filter(|tool| step.tools.includes(&tool.name))
            .map(|tool| tool.name.as_str())
    }

    /// The jobs that depend on each job, by ID.
    pub fn dependents(&self) -> BTreeMap<JobId, Vec<JobId>> {
        let mut dependents: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for job in &self.jobs {
            for &dependency in &job.depends_on {
                dependents.entry(dependency).or_default().push(job.id);
            }
        }
        dependents
    }

    /// The `.spitdag` document.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        // The text is kept whole, so there is nothing to hand on.
        let kept: io::Result<()> = write_document(self, &mut out, |_| Ok(()));
        debug_assert!(kept.is_ok());
        out
    }

    /// Write the `.spitdag` document to `writer` a piece at a time, without
    /// holding it all.
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
