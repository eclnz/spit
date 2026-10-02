//! A resolved DAG: its jobs over its artifact table, and the report of
//! what can and cannot be made.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::error::ResolveError;

use super::{stage_within, Artifact, ArtifactId, ArtifactInstance, Artifacts, SourceInventory};

/// A job's number in its DAG, counted from 1 in the order the resolver
/// makes jobs, as reports and the `.spitdag` show it. Unlike an
/// [`ArtifactId`], it is not an index: a DAG cut to one stage keeps its
/// jobs' numbers.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JobId(u32);

impl JobId {
    /// The job numbered `number`.
    pub(crate) fn new(number: usize) -> Self {
        Self(u32::try_from(number).expect("fewer than 2^32 jobs in a DAG"))
    }

    /// The job's number, from 1.
    pub fn number(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub id: JobId,
    pub operation: String,
    /// The artifacts bound to each input port, in port order. A many port
    /// holds its collection in order; every other port holds one artifact.
    pub inputs: Vec<Vec<ArtifactId>>,
    /// One artifact per output port, in port order.
    pub outputs: Vec<ArtifactId>,
    pub dependencies: Vec<JobId>,
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
    pub fn output(&self) -> ArtifactId {
        self.outputs[0]
    }

    /// Every input artifact, in port order.
    pub fn input_artifacts(&self) -> impl Iterator<Item = ArtifactId> + '_ {
        self.inputs.iter().flatten().copied()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedDag {
    pub jobs: Vec<Job>,
    /// Every artifact the jobs refer to.
    pub artifacts: Artifacts,
    /// Declaration order is retained for readable dry-run output.
    pub product_dimensions: BTreeMap<String, Vec<String>>,
    /// The file of each source whose inventory record gave one, by artifact
    /// id. Other artifacts take the path their product's rule gives them.
    pub(crate) source_paths: Vec<Option<String>>,
    /// The products of the inventory records that gave a file.
    pub(crate) located: BTreeSet<String>,
}

impl ResolvedDag {
    pub fn artifact(&self, id: ArtifactId) -> Artifact<'_> {
        self.artifacts.get(id)
    }

    /// Take each source's file from its record in `inventory`, replacing the
    /// files known before: a DAG resolved before its inventory's sources
    /// were located gets their files this way, without resolving it again.
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

    /// The file `id`'s inventory record gave it, if it is a source with one.
    pub fn source_path(&self, id: ArtifactId) -> Option<&str> {
        self.source_paths.get(id.index())?.as_deref()
    }

    /// The products with a source whose inventory record gave its file.
    pub fn located_products(&self) -> impl Iterator<Item = &str> {
        self.located.iter().map(String::as_str)
    }

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
            artifacts: self.artifacts.clone(),
            product_dimensions: self.product_dimensions.clone(),
            source_paths: self.source_paths.clone(),
            located: self.located.clone(),
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
    /// The inputs it did match, by id in the DAG's table, in port order.
    pub inputs: Vec<ArtifactId>,
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
    /// Every source, by product in declaration order.
    pub sources: Vec<ArtifactId>,
    pub dag: ResolvedDag,
    pub incomplete: Vec<IncompleteJob>,
    pub coverage: Vec<CoverageGap>,
}

impl ArtifactReport {
    /// The sources no job reads, complete or not, in source order: files
    /// the inventory holds that the plan has no use for, such as those a
    /// `where` selector leaves out, or a file whose name matches no value a
    /// job needs. Sources a coverage gap holds back are reported with the
    /// gap, not here.
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
