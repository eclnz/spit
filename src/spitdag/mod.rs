//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

mod write;

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use crate::model::{
    identity, natural_cmp, ArtifactId, ArtifactInstance, Artifacts, EntityBinding, JobId, Removal,
    StepId,
};
use crate::types::TypeExpr;

pub use crate::check::When;

use self::write::{write_document, PIECE};

/// The schema version a `.spitdag` is written with.
pub const SPITDAG_VERSION: usize = 6;

/// A resolved DAG with its paths bound and its commands expanded. Its
/// artifacts are the resolved DAG's, each kept once with its path; jobs,
/// and the paths in their commands, refer to them by id.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundDag {
    /// The absolute dataset folder every path is relative to, when known.
    pub root: Option<String>,
    /// What the input stage left out of the dataset, and why.
    pub removed: Vec<Removal>,
    /// Outputs whose jobs could not be planned, with the reasons from `artifacts`.
    pub left_out: Vec<LeftOut>,
    /// Each job after the jobs it depends on.
    pub jobs: Vec<BoundJob>,
    /// Each step of the resolved DAG, with its ports named, which jobs
    /// refer to by [`StepId`].
    pub steps: Vec<BoundStep>,
    artifacts: Artifacts,
    /// Each artifact's file, relative to the dataset root, by id; empty for
    /// an artifact no job uses.
    paths: Vec<String>,
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
    /// The command that makes the outputs; `None` when the operation has none.
    pub command: Option<Vec<Argument>>,
    /// Commands that check the inputs before the job runs.
    pub verify: Vec<Vec<Argument>>,
    /// The checks of single artifacts: those on inputs, then those on
    /// outputs.
    pub checks: Vec<BoundCheck>,
}

/// An artifact of a bound DAG, with its path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundArtifact<'a> {
    pub product: &'a str,
    pub artifact_type: &'a TypeExpr,
    /// The file or folder, relative to the dataset root.
    pub path: &'a str,
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
    /// An artifact's file, relative to the dataset root.
    Path(ArtifactId),
    /// The folder of an artifact's file, as `{image.dir}` gives it.
    Dir(ArtifactId),
    /// An artifact's file name without its extension, as `{image.stem}`
    /// gives it for a tool that adds the extension itself.
    Stem {
        artifact: ArtifactId,
        extension: String,
    },
}

impl ArgPart {
    /// The text this part is in a command, given the DAG it belongs to.
    pub fn text<'a>(&'a self, dag: &'a BoundDag) -> &'a str {
        match self {
            Self::Text(text) => text,
            Self::Path(artifact) => dag.path(*artifact),
            Self::Dir(artifact) => folder_of(dag.path(*artifact)),
            Self::Stem {
                artifact,
                extension,
            } => stem_of(dag.path(*artifact), extension),
        }
    }
}

/// The folder `path` is in, `.` for the root itself.
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(folder, _)| folder)
}

/// The file name of `path` without `extension`.
fn stem_of<'a>(path: &'a str, extension: &str) -> &'a str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    name.strip_suffix(extension).unwrap_or(name)
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
    /// `paths` holds for it and each product, by number, has `dimensions`
    /// and is a folder where `folders` says so.
    pub(crate) fn new(
        artifacts: Artifacts,
        paths: Vec<String>,
        dimensions: Vec<Vec<String>>,
        folders: Vec<bool>,
        steps: Vec<BoundStep>,
        jobs: Vec<BoundJob>,
    ) -> Self {
        Self {
            root: None,
            removed: Vec::new(),
            left_out: Vec::new(),
            jobs,
            steps,
            artifacts,
            paths,
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
            folder: self.folders[product],
            entities: artifact.entities,
            dimensions: &self.dimensions[product],
        }
    }

    /// The path of `id`, relative to the dataset root.
    pub fn path(&self, id: ArtifactId) -> &str {
        &self.paths[id.index()]
    }

    /// Whether each artifact, by id, is an output of a job here.
    fn produced(&self) -> Vec<bool> {
        let mut produced = vec![false; self.paths.len()];
        for &output in self.jobs.iter().flat_map(|job| &job.outputs) {
            produced[output.index()] = true;
        }
        produced
    }

    /// The inputs no job here makes, each once, by path: sources, and the
    /// outputs of stages left out.
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
    /// argument that is plain text, so a backend can look for them before it
    /// starts.
    pub fn executables(&self) -> BTreeSet<String> {
        self.jobs
            .iter()
            .flat_map(|job| {
                let checks = job.checks.iter().map(|check| &check.command);
                job.command.iter().chain(&job.verify).chain(checks)
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
            .collect()
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
