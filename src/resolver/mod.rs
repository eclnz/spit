//! Resolve a pipeline against a source inventory: check everything the
//! pipeline text determines, then expand each step, in dependency order, into
//! concrete jobs over the inventory's artifacts.

mod definitions;
mod matching;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{
    ArtifactInstance, ArtifactKey, ArtifactReport, Gap, IncompleteJob, Invocation, OperationDef,
    Pipeline, ProductDef, ResolvedDag, SourceInventory,
};
use crate::types::TypeExpr;

use self::definitions::{
    check_stages, index_operations, index_producers, index_products, invocation_order,
};
use self::matching::{expand_step, make_job, step_shape, Expansion, StepShape};
use self::types::infer_types;

/// A pipeline whose declarations, steps, and rules hold without any inventory.
struct CheckedPipeline<'a> {
    products: BTreeMap<&'a str, &'a ProductDef>,
    operations: BTreeMap<&'a str, &'a OperationDef>,
    /// Invocation indices with every producer before its consumers.
    order: Vec<usize>,
    /// The statically inferred type of each checked step's outputs.
    inferred_types: BTreeMap<String, TypeExpr>,
    /// How each checked step, by invocation index, shapes its jobs.
    shapes: BTreeMap<usize, StepShape>,
}

/// Every pipeline error, plus the names that failed or depend on a failure.
pub(crate) struct PipelineCheck<'a> {
    pipeline: CheckedPipeline<'a>,
    pub errors: Vec<(DefinitionSubject, ResolveError)>,
    /// Products and operations that are invalid or produced by a step that
    /// could not be checked. Anything using them is skipped rather than
    /// reported again.
    pub poisoned: BTreeSet<String>,
}

/// Check everything that depends only on the pipeline text: declarations,
/// each step's operation, inputs, dimensions and inferred types, and cycles.
/// Returns the first error.
pub fn validate_pipeline(pipeline: &Pipeline) -> Result<(), ResolveError> {
    check_pipeline(pipeline).map(|_| ())
}

fn check_pipeline(pipeline: &Pipeline) -> Result<CheckedPipeline<'_>, ResolveError> {
    let checked = collect_pipeline(pipeline);
    match checked.errors.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(checked.pipeline),
    }
}

/// Check the whole pipeline without an inventory, collecting every error.
pub(crate) fn collect_pipeline(pipeline: &Pipeline) -> PipelineCheck<'_> {
    let mut errors = Vec::new();
    let mut poisoned = BTreeSet::new();
    let products = index_products(
        &pipeline.products,
        &pipeline.invocations,
        &mut errors,
        &mut poisoned,
    );
    let operations = index_operations(&pipeline.operations, &mut errors, &mut poisoned);
    let producers = index_producers(
        &pipeline.invocations,
        &products,
        &operations,
        &mut errors,
        &mut poisoned,
    );
    check_stages(pipeline, &producers, &mut errors);
    let order = match invocation_order(&pipeline.invocations, &producers) {
        Ok(order) => order,
        Err(error) => {
            let ResolveError::Cycle { products } = &error else {
                unreachable!("ordering only reports cycles")
            };
            errors.push((DefinitionSubject::Invocation(products[0].clone()), error));
            (0..pipeline.invocations.len()).collect()
        }
    };
    // Inferred intermediate types are also part of the reusable pipeline
    // contract. Check them in dependency order so a type error does not depend
    // on whether an inventory happens to contain concrete source artifacts.
    // A step that fails, or uses something that failed, quiets its consumers.
    let mut inferred_types = BTreeMap::new();
    let mut shapes = BTreeMap::new();
    for &index in &order {
        let invocation = &pipeline.invocations[index];
        let depends_on_failure = poisoned.contains(&invocation.operation)
            || invocation
                .outputs
                .iter()
                .any(|output| poisoned.contains(output))
            || invocation
                .inputs
                .iter()
                .any(|input| poisoned.contains(input.product_name()));
        if depends_on_failure {
            poisoned.extend(invocation.outputs.iter().cloned());
            continue;
        }
        match validate_invocation(invocation, &products, &operations, &inferred_types) {
            Ok((inferred, shape)) => {
                inferred_types.extend(invocation.outputs.iter().cloned().zip(inferred));
                shapes.insert(index, shape);
            }
            Err(error) => {
                errors.push((
                    DefinitionSubject::Invocation(invocation.output_product().to_owned()),
                    error,
                ));
                poisoned.extend(invocation.outputs.iter().cloned());
            }
        }
    }
    PipelineCheck {
        pipeline: CheckedPipeline {
            products,
            operations,
            order,
            inferred_types,
            shapes,
        },
        errors,
        poisoned,
    }
}

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
    let CheckedPipeline {
        products,
        operations,
        order,
        inferred_types,
        shapes,
    } = check_pipeline(pipeline)?;
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

pub(super) fn find_product<'a>(
    products: &'a BTreeMap<&str, &ProductDef>,
    name: &str,
) -> Result<&'a ProductDef, ResolveError> {
    products
        .get(name)
        .copied()
        .ok_or_else(|| ResolveError::UnknownProduct {
            name: name.to_owned(),
        })
}

pub(super) fn find_operation<'a>(
    operations: &'a BTreeMap<&str, &OperationDef>,
    name: &str,
) -> Result<&'a OperationDef, ResolveError> {
    operations
        .get(name)
        .copied()
        .ok_or_else(|| ResolveError::UnknownOperation {
            name: name.to_owned(),
        })
}

fn validate_invocation(
    invocation: &Invocation,
    products: &BTreeMap<&str, &ProductDef>,
    operations: &BTreeMap<&str, &OperationDef>,
    inferred_types: &BTreeMap<String, TypeExpr>,
) -> Result<(Vec<TypeExpr>, StepShape), ResolveError> {
    let operation = find_operation(operations, &invocation.operation)?;
    let outputs = invocation
        .outputs
        .iter()
        .map(|name| find_product(products, name))
        .collect::<Result<Vec<_>, _>>()?;
    if invocation.inputs.len() != operation.inputs.len() {
        return Err(unsupported(
            operation,
            format!(
                "expected {} input bindings, found {}",
                operation.inputs.len(),
                invocation.inputs.len()
            ),
        ));
    }
    if invocation.outputs.len() != operation.outputs.len() {
        let names: Vec<_> = operation
            .outputs
            .iter()
            .map(|port| port.name.as_str())
            .collect();
        return Err(unsupported(
            operation,
            format!(
                "expected {} output products ({}), found {}",
                operation.outputs.len(),
                names.join(", "),
                invocation.outputs.len()
            ),
        ));
    }
    let inferred = infer_types(invocation, operation, products, &outputs, inferred_types)?;
    let shape = step_shape(invocation, operation, products, &outputs)?;
    Ok((inferred, shape))
}

pub(super) fn family<'a>(
    artifacts: &'a BTreeMap<String, Vec<ArtifactInstance>>,
    product: &str,
) -> &'a [ArtifactInstance] {
    artifacts.get(product).map_or(&[], Vec::as_slice)
}

pub(super) fn unsupported(operation: &OperationDef, detail: impl Into<String>) -> ResolveError {
    ResolveError::UnsupportedShapeRelationship {
        operation: operation.name.clone(),
        detail: detail.into(),
    }
}
