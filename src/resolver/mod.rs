//! Resolve jobs: expand each step of a compiled pipeline, in dependency
//! order, into concrete jobs over an inventory's artifacts.

mod bind;
mod matching;

use crate::error::ResolveError;
use crate::model::{
    ArtifactId, ArtifactInstance, ArtifactReport, ArtifactType, Gap, IncompleteJob, Invocation,
    OperationDef, Pipeline, ResolvedDag, SourceInventory,
};

use crate::compile::{compile, CompiledPipeline};

pub use self::bind::{bind_dag, bind_dag_with, BindError};
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
    let mut resolution = Resolution {
        families: Vec::new(),
        incomplete: Vec::new(),
        producers: Vec::new(),
        dag: ResolvedDag {
            product_dimensions: pipeline
                .products
                .iter()
                .map(|product| (product.name.clone(), product.dimensions.clone()))
                .collect(),
            ..ResolvedDag::default()
        },
        incomplete_jobs: Vec::new(),
    };
    pipeline.check_sources(inventory, |product, record| {
        let number = resolution.product(&product.name, &product.artifact_type);
        match resolution
            .dag
            .artifacts
            .add(number, record.entities.clone())
        {
            Ok(id) => {
                resolution.families[number as usize].push(id);
                true
            }
            Err(_) => false,
        }
    })?;
    // Sources in natural order, by product in declaration order.
    let mut sources = Vec::new();
    for product in &pipeline.products {
        if let Some(number) = resolution.dag.artifacts.product_number(&product.name) {
            let artifacts = &resolution.dag.artifacts;
            let family = &mut resolution.families[number as usize];
            family.sort_by(|&left, &right| {
                artifacts
                    .entities(left)
                    .cmp_in(artifacts.entities(right), &product.dimensions)
            });
            sources.extend_from_slice(family);
        }
    }
    resolution.grow();
    for artifact in unavailable {
        if let Some(id) = resolution
            .dag
            .artifacts
            .find(&artifact.product, &artifact.entities)
        {
            resolution.incomplete[id.index()] = true;
        }
    }
    for step in &steps {
        let outputs: Vec<u32> = step
            .outputs
            .iter()
            .map(|(product, artifact_type)| resolution.product(&product.name, artifact_type))
            .collect();
        let expansions = expand_step(
            step,
            &resolution.dag.artifacts,
            &resolution.families,
            &resolution.incomplete,
        );
        for expansion in expansions {
            resolution.add(step.invocation, step.operation, &outputs, expansion)?;
        }
        for ((product, _), &number) in step.outputs.iter().zip(&outputs) {
            let artifacts = &resolution.dag.artifacts;
            resolution.families[number as usize].sort_by(|&left, &right| {
                artifacts
                    .entities(left)
                    .cmp_in(artifacts.entities(right), &product.dimensions)
            });
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
/// Artifacts are kept in the DAG's table; the rest is by artifact id or
/// product number.
struct Resolution {
    /// Each product's artifacts, by product number, in natural order.
    families: Vec<Vec<ArtifactId>>,
    /// Artifacts that will not exist: sources the caller rules out, and
    /// the outputs of incomplete jobs.
    incomplete: Vec<bool>,
    /// The job that makes each output artifact.
    producers: Vec<Option<usize>>,
    dag: ResolvedDag,
    incomplete_jobs: Vec<IncompleteJob>,
}

impl Resolution {
    /// The number of `product`, with a family for it.
    fn product(&mut self, product: &str, artifact_type: &ArtifactType) -> u32 {
        let number = self.dag.artifacts.product(product, artifact_type);
        if self.families.len() <= number as usize {
            self.families.resize_with(number as usize + 1, Vec::new);
        }
        number
    }

    /// Give every artifact in the table its flags.
    fn grow(&mut self) {
        let len = self.dag.artifacts.len();
        self.incomplete.resize(len, false);
        self.producers.resize(len, None);
    }

    /// Record one expanded job: a complete job joins the DAG, and one with
    /// gaps is reported with its outputs marked incomplete.
    fn add(
        &mut self,
        invocation: &Invocation,
        operation: &OperationDef,
        products: &[u32],
        expansion: Expansion,
    ) -> Result<(), ResolveError> {
        let mut outputs = Vec::with_capacity(products.len());
        for &product in products {
            let id = self
                .dag
                .artifacts
                .add(product, expansion.context.clone())
                .map_err(|existing| ResolveError::DuplicateOutputArtifact {
                    artifact: self.dag.artifacts.get(existing).to_instance(),
                })?;
            self.families[product as usize].push(id);
            outputs.push(id);
        }
        self.grow();
        if expansion.gaps.is_empty() {
            let job = make_job(
                self.dag.jobs.len() + 1,
                operation,
                invocation.stage.clone(),
                expansion.inputs,
                outputs,
                &self.producers,
            );
            for output in &job.outputs {
                self.producers[output.index()] = Some(job.id);
            }
            self.dag.jobs.push(job);
        } else {
            for output in &outputs {
                self.incomplete[output.index()] = true;
            }
            self.incomplete_jobs.push(IncompleteJob {
                operation: operation.name.clone(),
                stage: invocation.stage.clone(),
                outputs: outputs
                    .iter()
                    .map(|&id| self.dag.artifacts.get(id).to_instance())
                    .collect(),
                gaps: expansion.gaps,
            });
        }
        Ok(())
    }
}
