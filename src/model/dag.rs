use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::ResolveError;

use super::{
    Artifact, ArtifactId, ArtifactInstance, Artifacts, CallId, SourceInventory, StepOrigin,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JobId(u32);

impl JobId {
    pub(crate) fn new(number: usize) -> Self {
        Self(u32::try_from(number).expect("fewer than 2^32 jobs in a DAG"))
    }

    pub fn number(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StepId(u32);

impl StepId {
    pub(crate) fn new(index: usize) -> Self {
        Self(u32::try_from(index).expect("fewer than 2^32 steps in a pipeline"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DagStep {
    pub operation: String,
    pub outputs: Vec<String>,
    pub stage: Option<String>,
    pub origin: Option<StepOrigin>,
    pub line: Option<usize>,
    pub collection: Option<CollectionRule>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionRule {
    pub ports: Vec<usize>,
    pub member: Vec<String>,
    pub minimum: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub id: JobId,
    pub step: StepId,
    pub inputs: Vec<Vec<ArtifactId>>,
    pub outputs: Vec<ArtifactId>,
    pub dependencies: Vec<JobId>,
    pub missing: Vec<MissingInput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingInput {
    pub message: String,
    pub member: Option<Vec<String>>,
}

impl Job {
    pub fn output(&self) -> ArtifactId {
        self.outputs[0]
    }

    pub fn input_artifacts(&self) -> impl Iterator<Item = ArtifactId> + '_ {
        self.inputs.iter().flatten().copied()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedDag {
    pub jobs: Vec<Job>,
    pub steps: Vec<DagStep>,
    pub artifacts: Artifacts,
    pub views: BTreeMap<ArtifactId, Vec<ArtifactId>>,
    pub product_dimensions: BTreeMap<String, Vec<String>>,
    pub(crate) source_paths: Vec<Option<String>>,
    pub(crate) located: BTreeSet<String>,
}

impl ResolvedDag {
    pub fn step(&self, job: &Job) -> &DagStep {
        &self.steps[job.step.index()]
    }

    pub fn artifact(&self, id: ArtifactId) -> Artifact<'_> {
        self.artifacts.get(id)
    }

    pub fn locate_sources(&mut self, inventory: &SourceInventory) {
        self.source_paths = vec![None; self.artifacts.len()];
        self.located.clear();
        for record in &inventory.artifacts {
            let Some(path) = &record.path else {
                continue;
            };
            if !self.located.contains(&record.product) {
                self.located.insert(record.product.clone());
            }
            if let Some(id) = self.artifacts.find(&record.product, &record.entities) {
                self.source_paths[id.index()] = Some(path.clone());
            }
        }
    }

    pub fn source_path(&self, id: ArtifactId) -> Option<&str> {
        self.source_paths.get(id.index())?.as_deref()
    }

    pub fn located_products(&self) -> impl Iterator<Item = &str> {
        self.located.iter().map(String::as_str)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Gap {
    Unmatched(ResolveError),
    Blocked {
        port: String,
        artifact: ArtifactInstance,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncompleteJob {
    pub operation: String,
    pub stage: Option<String>,
    pub inputs: Vec<ArtifactId>,
    pub outputs: Vec<ArtifactInstance>,
    pub gaps: Vec<Gap>,
    pub call: Option<CallId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageGap {
    pub error: ResolveError,
    pub sources: Vec<ArtifactInstance>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArtifactReport {
    pub sources: Vec<ArtifactId>,
    pub dag: ResolvedDag,
    pub incomplete: Vec<IncompleteJob>,
    pub coverage: Vec<CoverageGap>,
}

impl ArtifactReport {
    pub fn unused_sources(&self) -> Vec<ArtifactId> {
        let mut read = vec![false; self.dag.artifacts.len()];
        let complete = self.dag.jobs.iter().flat_map(Job::input_artifacts);
        let incomplete = self
            .incomplete
            .iter()
            .flat_map(|job| job.inputs.iter().copied());
        for id in complete.chain(incomplete) {
            read[id.index()] = true;
        }
        for source in self.coverage.iter().flat_map(|gap| &gap.sources) {
            if let Some(id) = self.dag.artifacts.find(&source.product, &source.entities) {
                read[id.index()] = true;
            }
        }
        self.sources
            .iter()
            .copied()
            .filter(|id| !read[id.index()])
            .collect()
    }
}
