//! Resolve jobs: expand each step of a compiled pipeline, in dependency
//! order, into concrete jobs over an inventory's artifacts.

mod bind;
mod matching;

use std::collections::BTreeMap;

use crate::error::ResolveError;
use crate::model::{
    ArtifactInstance, ArtifactMap, ArtifactReport, ArtifactSet, Gap, IncompleteJob, Invocation,
    OperationDef, Pipeline, ResolvedDag, SourceInventory,
};

use crate::compile::{compile, CompiledPipeline};

pub use self::bind::{bind_dag, BindError};
use self::matching::{expand_step, make_job, Expansion};

/// Resolve every job of `pipeline` over the sources in `inventory`, failing
/// on the first job that cannot be made.
pub fn resolve(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ResolvedDag, ResolveError> {
    let report = resolve_artifacts_excluding(pipeline, inventory, &[])?;
    match first_failure(&report.incomplete) {
        Some(error) => Err(error),
        None => Ok(report.dag),
    }
}

/// Why the first of `incomplete` cannot be made; a blocked gap always
/// follows the gap that blocks it, so there is one whenever any job is
/// incomplete.
pub(crate) fn first_failure(incomplete: &[IncompleteJob]) -> Option<ResolveError> {
    incomplete
        .iter()
        .flat_map(|job| &job.gaps)
        .find_map(|gap| match gap {
            Gap::Unmatched(error) => Some(error.clone()),
            Gap::Blocked { .. } => None,
        })
}

/// Resolve what can be made, reporting each job that cannot and why. Some
/// sources may be known to be unusable, such as those a missing requirement
/// holds back; jobs that need them are reported as blocked.
pub fn resolve_artifacts_excluding(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    unavailable: &[ArtifactInstance],
) -> Result<ArtifactReport, ResolveError> {
    let CompiledPipeline { steps } = compile(pipeline)?;
    let artifacts = pipeline.source_artifacts(inventory)?;
    let sources = pipeline
        .products
        .iter()
        .flat_map(|product| family(&artifacts, &product.name))
        .cloned()
        .collect();
    let mut resolution = Resolution {
        seen: artifacts.values().flatten().collect(),
        incomplete: unavailable.iter().collect(),
        artifacts,
        producers: ArtifactMap::default(),
        dag: ResolvedDag {
            jobs: Vec::new(),
            product_dimensions: pipeline
                .products
                .iter()
                .map(|product| (product.name.clone(), product.dimensions.clone()))
                .collect(),
            source_paths: BTreeMap::new(),
        },
        incomplete_jobs: Vec::new(),
    };
    for step in &steps {
        let expansions = expand_step(step, &resolution.artifacts, &resolution.incomplete);
        for expansion in expansions {
            resolution.add(step.invocation, step.operation, expansion)?;
        }
        for (product, _) in &step.outputs {
            if let Some(family) = resolution.artifacts.get_mut(&product.name) {
                product.sort_family(family);
            }
        }
    }
    let mut dag = resolution.dag;
    dag.locate_sources(inventory);
    Ok(ArtifactReport {
        sources,
        dag,
        incomplete: resolution.incomplete_jobs,
        coverage: Vec::new(),
    })
}

/// The artifacts and jobs found so far, as steps are expanded in order.
struct Resolution {
    artifacts: BTreeMap<String, Vec<ArtifactInstance>>,
    /// Every artifact, so that none is made twice.
    seen: ArtifactSet,
    /// Artifacts that will not exist: sources the caller rules out, and
    /// the outputs of incomplete jobs.
    incomplete: ArtifactSet,
    /// The job that makes each output artifact.
    producers: ArtifactMap<usize>,
    dag: ResolvedDag,
    incomplete_jobs: Vec<IncompleteJob>,
}

impl Resolution {
    /// Record one expanded job: a complete job joins the DAG, and one with
    /// gaps is reported with its outputs marked incomplete.
    fn add(
        &mut self,
        invocation: &Invocation,
        operation: &OperationDef,
        expansion: Expansion,
    ) -> Result<(), ResolveError> {
        for output in &expansion.outputs {
            if !self.seen.add(output) {
                return Err(ResolveError::DuplicateOutputArtifact {
                    artifact: output.clone(),
                });
            }
            self.artifacts
                .entry(output.product.clone())
                .or_default()
                .push(output.clone());
        }
        if expansion.gaps.is_empty() {
            let job = make_job(
                self.dag.jobs.len() + 1,
                operation,
                invocation.stage.clone(),
                expansion.inputs,
                expansion.outputs,
                &self.producers,
            );
            for output in &job.outputs {
                self.producers.insert(output, job.id);
            }
            self.dag.jobs.push(job);
        } else {
            for output in &expansion.outputs {
                self.incomplete.add(output);
            }
            self.incomplete_jobs.push(IncompleteJob {
                operation: operation.name.clone(),
                stage: invocation.stage.clone(),
                outputs: expansion.outputs,
                gaps: expansion.gaps,
            });
        }
        Ok(())
    }
}

pub(super) fn family<'a>(
    artifacts: &'a BTreeMap<String, Vec<ArtifactInstance>>,
    product: &str,
) -> &'a [ArtifactInstance] {
    artifacts.get(product).map_or(&[], Vec::as_slice)
}
