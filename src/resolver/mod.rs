//! Resolve jobs: expand each step of a compiled pipeline, in dependency
//! order, into concrete jobs over an inventory's artifacts.

mod matching;

use std::collections::{BTreeMap, BTreeSet};

use crate::error::ResolveError;
use crate::model::{
    ArtifactInstance, ArtifactKey, ArtifactReport, Gap, IncompleteJob, Invocation, OperationDef,
    Pipeline, ResolvedDag, SourceInventory,
};

use crate::compile::{compile, CompiledPipeline};

use self::matching::{expand_step, make_job, Expansion};

/// Resolve every job of `pipeline` over the sources in `inventory`, failing
/// on the first job that cannot be made.
pub fn resolve(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ResolvedDag, ResolveError> {
    let report = resolve_artifacts(pipeline, inventory)?;
    // A blocked gap always follows the gap that blocks it.
    let failure = report
        .incomplete
        .into_iter()
        .flat_map(|job| job.gaps)
        .find_map(|gap| match gap {
            Gap::Unmatched(error) => Some(error),
            Gap::Blocked { .. } => None,
        });
    match failure {
        Some(error) => Err(error),
        None => Ok(report.dag),
    }
}

/// Resolve what can be made, reporting each job that cannot and why.
pub fn resolve_artifacts(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ArtifactReport, ResolveError> {
    resolve_artifacts_excluding(pipeline, inventory, &[])
}

/// As [`resolve_artifacts`], with some listed sources known to be unusable,
/// such as those an input rule holds back. Jobs that need them are reported
/// as blocked.
pub fn resolve_artifacts_excluding(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    unavailable: &[ArtifactInstance],
) -> Result<ArtifactReport, ResolveError> {
    let CompiledPipeline {
        products,
        operations,
        order,
        inferred_types,
        shapes,
    } = compile(pipeline)?;
    let artifacts = pipeline.source_artifacts(inventory)?;
    let sources = pipeline
        .products
        .iter()
        .flat_map(|product| family(&artifacts, &product.name))
        .cloned()
        .collect();
    let mut resolution = Resolution {
        seen: artifacts
            .values()
            .flatten()
            .map(ArtifactInstance::key)
            .collect(),
        incomplete: unavailable.iter().map(ArtifactInstance::key).collect(),
        artifacts,
        producers: BTreeMap::new(),
        dag: ResolvedDag {
            jobs: Vec::new(),
            product_dimensions: pipeline
                .products
                .iter()
                .map(|product| (product.name.clone(), product.dimensions.clone()))
                .collect(),
        },
        incomplete_jobs: Vec::new(),
    };
    for index in order {
        let invocation = &pipeline.invocations[index];
        let operation = operations[invocation.operation.as_str()];
        // Every artifact in a family has the same type, so the type inferred
        // statically for each output is the type of each job's artifact.
        let outputs: Vec<_> = invocation
            .outputs
            .iter()
            .map(|name| (products[name.as_str()], &inferred_types[name]))
            .collect();
        let expansions = expand_step(
            invocation,
            operation,
            &shapes[&index],
            &outputs,
            &resolution.artifacts,
            &resolution.incomplete,
        );
        for expansion in expansions {
            resolution.add(invocation, operation, expansion)?;
        }
        for (product, _) in &outputs {
            if let Some(family) = resolution.artifacts.get_mut(&product.name) {
                product.sort_family(family);
            }
        }
    }
    Ok(ArtifactReport {
        sources,
        dag: resolution.dag,
        incomplete: resolution.incomplete_jobs,
        coverage: Vec::new(),
    })
}

/// The artifacts and jobs found so far, as steps are expanded in order.
struct Resolution {
    artifacts: BTreeMap<String, Vec<ArtifactInstance>>,
    /// Every artifact, so that none is made twice.
    seen: BTreeSet<ArtifactKey>,
    /// Artifacts that will not exist: sources the caller rules out, and
    /// the outputs of incomplete jobs.
    incomplete: BTreeSet<ArtifactKey>,
    /// The job that makes each output artifact.
    producers: BTreeMap<ArtifactKey, usize>,
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
            if !self.seen.insert(output.key()) {
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
                self.producers.insert(output.key(), job.id);
            }
            self.dag.jobs.push(job);
        } else {
            self.incomplete
                .extend(expansion.outputs.iter().map(ArtifactInstance::key));
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
