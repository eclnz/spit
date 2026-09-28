//! Resolve a pipeline against a source inventory: check everything the
//! pipeline text determines, then expand each step, in dependency order, into
//! concrete jobs over the inventory's artifacts.

mod coverage;
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

use self::coverage::{check_coverage_rule, coverage_gaps};
use self::definitions::{
    check_stages, index_operations, index_producers, index_products, invocation_order,
};
use self::matching::{expand_step, make_job, step_shape, Expansion, StepShape};
use self::types::infer_types;

/// A pipeline whose declarations, steps, and rules hold without any inventory.
struct CheckedPipeline<'a> {
    products: BTreeMap<&'a str, &'a ProductDef>,
    operations: BTreeMap<&'a str, &'a OperationDef>,
    producers: BTreeMap<String, usize>,
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
/// each step's operation, inputs, dimensions and inferred types, cycles, and
/// the shape of coverage rules. Returns the first error.
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
    for (index, rule) in pipeline.constraints.iter().enumerate() {
        if poisoned.contains(&rule.product) {
            continue;
        }
        if let Err(error) = check_coverage_rule(index, rule, &products, &producers) {
            // Keep the more specific subject a definition error names.
            let subject = match &error {
                ResolveError::InvalidDefinition { subject, .. } => subject.clone(),
                _ => DefinitionSubject::Constraint(index),
            };
            errors.push((subject, error));
        }
    }
    PipelineCheck {
        pipeline: CheckedPipeline {
            products,
            operations,
            producers,
            order,
            inferred_types,
            shapes,
        },
        errors,
        poisoned,
    }
}

pub fn resolve(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ResolvedDag, ResolveError> {
    let report = resolve_artifacts(pipeline, inventory)?;
    // A blocked gap always follows the gap that blocks it.
    let unmatched = report
        .incomplete
        .into_iter()
        .flat_map(|job| job.gaps)
        .filter_map(|gap| match gap {
            Gap::Unmatched(error) => Some(error),
            Gap::Blocked { .. } => None,
        });
    let failure = report
        .coverage
        .into_iter()
        .map(|gap| gap.error)
        .chain(unmatched)
        .next();
    match failure {
        Some(error) => Err(error),
        None => Ok(report.dag),
    }
}

pub fn resolve_artifacts(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ArtifactReport, ResolveError> {
    let CheckedPipeline {
        products,
        operations,
        producers,
        order,
        inferred_types,
        shapes,
    } = check_pipeline(pipeline)?;
    let artifacts = source_artifacts(inventory, &products, &producers)?;
    let coverage: Vec<_> = pipeline
        .constraints
        .iter()
        .enumerate()
        .flat_map(|(rule_index, rule)| coverage_gaps(rule_index, rule, inventory, &artifacts))
        .collect();
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
        incomplete: coverage
            .iter()
            .flat_map(|gap| &gap.sources)
            .map(ArtifactInstance::key)
            .collect(),
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
                sort_family(family, product);
            }
        }
    }
    Ok(ArtifactReport {
        sources,
        dag: resolution.dag,
        incomplete: resolution.incomplete_jobs,
        coverage,
    })
}

/// Each source artifact in the inventory, by product, after checking that
/// it binds its product's dimensions and appears once.
fn source_artifacts(
    inventory: &SourceInventory,
    products: &BTreeMap<&str, &ProductDef>,
    producers: &BTreeMap<String, usize>,
) -> Result<BTreeMap<String, Vec<ArtifactInstance>>, ResolveError> {
    let mut artifacts: BTreeMap<String, Vec<ArtifactInstance>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for record in &inventory.artifacts {
        let product = find_product(products, &record.product)?;
        if producers.contains_key(&record.product) {
            return Err(ResolveError::InvalidDefinition {
                subject: DefinitionSubject::Product(record.product.clone()),
                detail: format!(
                    "product `{}` cannot be both a source family and an invocation output",
                    record.product
                ),
            });
        }
        let source = ArtifactInstance {
            product: record.product.clone(),
            artifact_type: product.artifact_type.clone(),
            entities: record.entities.clone(),
        };
        let actual: BTreeSet<_> = record.entities.0.keys().cloned().collect();
        let expected: BTreeSet<_> = product.dimensions.iter().cloned().collect();
        if actual != expected {
            return Err(ResolveError::InvalidDefinition {
                subject: DefinitionSubject::Source(record.clone()),
                detail: format!(
                    "source `{source}` must bind exactly the dimensions of product `{}`: [{}]",
                    product.name,
                    product.dimensions.join(", ")
                ),
            });
        }
        if !seen.insert(source.key()) {
            return Err(ResolveError::DuplicateSourceArtifact { artifact: source });
        }
        artifacts
            .entry(source.product.clone())
            .or_default()
            .push(source);
    }
    for (name, family) in &mut artifacts {
        sort_family(family, products[name.as_str()]);
    }
    Ok(artifacts)
}

/// The artifacts and jobs found so far, as steps are expanded in order.
struct Resolution {
    artifacts: BTreeMap<String, Vec<ArtifactInstance>>,
    /// Every artifact, so that none is made twice.
    seen: BTreeSet<ArtifactKey>,
    /// Artifacts that will not exist: sources a coverage rule rejects, and
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

/// Order a family by its declared dimensions, reading numbers as numbers.
fn sort_family(family: &mut [ArtifactInstance], product: &ProductDef) {
    family.sort_by(|left, right| left.entities.cmp_in(&right.entities, &product.dimensions));
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
