use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::error::{DefinitionSubject, ResolveError, TypeConflict};
use crate::model::{
    ArtifactInstance, ArtifactKey, ArtifactReport, Cardinality, CountRequirement, CoverageGap,
    CoverageRule, EntityBinding, Gap, IncompleteJob, InputBinding, InputPort, Invocation, Job,
    OperationDef, Pipeline, ProductDef, ResolvedDag, ShapeRule, SourceInventory,
};
use crate::types::{Substitutions, TypeExpr, TypeUnifyError};

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
    let mut artifacts: BTreeMap<String, Vec<ArtifactInstance>> = BTreeMap::new();
    let mut artifact_producers: BTreeMap<ArtifactKey, usize> = BTreeMap::new();
    let mut seen: BTreeSet<ArtifactKey> = BTreeSet::new();

    for record in &inventory.artifacts {
        let product = find_product(&products, &record.product)?;
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
        let key = source.key();
        if !seen.insert(key) {
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
    let coverage: Vec<_> = pipeline
        .constraints
        .iter()
        .enumerate()
        .flat_map(|(rule_index, rule)| coverage_gaps(rule_index, rule, inventory, &artifacts))
        .collect();
    let mut incomplete: BTreeSet<ArtifactKey> = coverage
        .iter()
        .flat_map(|gap| &gap.sources)
        .map(ArtifactInstance::key)
        .collect();
    let sources = pipeline
        .products
        .iter()
        .flat_map(|product| family(&artifacts, &product.name))
        .cloned()
        .collect();

    let mut incomplete_jobs = Vec::new();
    let mut dag = ResolvedDag {
        jobs: Vec::new(),
        product_dimensions: pipeline
            .products
            .iter()
            .map(|product| (product.name.clone(), product.dimensions.clone()))
            .collect(),
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
            &artifacts,
            &incomplete,
        );
        for expansion in expansions {
            for output in &expansion.outputs {
                if !seen.insert(output.key()) {
                    return Err(ResolveError::DuplicateOutputArtifact {
                        artifact: output.clone(),
                    });
                }
                artifacts
                    .entry(output.product.clone())
                    .or_default()
                    .push(output.clone());
            }
            if expansion.gaps.is_empty() {
                let job = make_job(
                    dag.jobs.len() + 1,
                    operation,
                    invocation.stage.clone(),
                    expansion.inputs,
                    expansion.outputs,
                    &artifact_producers,
                );
                for output in &job.outputs {
                    artifact_producers.insert(output.key(), job.id);
                }
                dag.jobs.push(job);
            } else {
                incomplete.extend(expansion.outputs.iter().map(ArtifactInstance::key));
                incomplete_jobs.push(IncompleteJob {
                    operation: operation.name.clone(),
                    stage: invocation.stage.clone(),
                    outputs: expansion.outputs,
                    gaps: expansion.gaps,
                });
            }
        }
        for (product, _) in &outputs {
            if let Some(family) = artifacts.get_mut(&product.name) {
                sort_family(family, product);
            }
        }
    }
    Ok(ArtifactReport {
        sources,
        dag,
        incomplete: incomplete_jobs,
        coverage,
    })
}

/// Order a family by its declared dimensions, reading numbers as numbers.
fn sort_family(family: &mut [ArtifactInstance], product: &ProductDef) {
    family.sort_by(|left, right| left.entities.cmp_in(&right.entities, &product.dimensions));
}

fn check_coverage_rule(
    rule_index: usize,
    rule: &CoverageRule,
    products: &BTreeMap<&str, &ProductDef>,
    producers: &BTreeMap<String, usize>,
) -> Result<(), ResolveError> {
    let product = find_product(products, &rule.product)?;
    if producers.contains_key(&rule.product) {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Constraint(rule_index),
            detail: format!(
                "coverage rule product `{}` must be a source family",
                rule.product
            ),
        });
    }
    let group_by = dimension_set(&rule.group_by);
    if group_by.len() != rule.group_by.len()
        || !group_by.is_subset(&dimension_set(&product.dimensions))
    {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::ConstraintGroup(rule_index),
            detail: format!(
                "coverage rule for `{}` must group by distinct dimensions of that product",
                rule.product
            ),
        });
    }
    for dimension in rule.values.keys() {
        if !product.dimensions.contains(dimension) || group_by.contains(dimension) {
            return Err(ResolveError::InvalidDefinition {
                subject: DefinitionSubject::Constraint(rule_index),
                detail: format!(
                    "coverage rule for `{}` requires values of `{dimension}`, which must be a dimension of that product outside its groups",
                    rule.product
                ),
            });
        }
    }
    Ok(())
}

fn coverage_gaps(
    rule_index: usize,
    rule: &CoverageRule,
    inventory: &SourceInventory,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
) -> Vec<CoverageGap> {
    let groups: BTreeSet<_> = inventory
        .contexts
        .iter()
        .chain(inventory.artifacts.iter().map(|record| &record.entities))
        .filter_map(|binding| binding.project(&rule.group_by))
        .collect();
    let mut gaps = Vec::new();
    for context in groups {
        let members: Vec<_> = family(artifacts, &rule.product)
            .iter()
            .filter(|artifact| artifact.entities.project(&rule.group_by).as_ref() == Some(&context))
            .cloned()
            .collect();
        let found = members.len();
        let valid = match rule.count {
            CountRequirement::Exactly(expected) => found == expected,
            CountRequirement::AtLeast(minimum) => found >= minimum,
        };
        let mut errors = Vec::new();
        if !valid {
            errors.push(ResolveError::CoverageViolation {
                product: rule.product.clone(),
                rule_index,
                context: context.clone(),
                expected: rule.count.clone(),
                found,
            });
        }
        for (dimension, values) in &rule.values {
            let missing = values.iter().filter(|value| {
                !members
                    .iter()
                    .any(|artifact| artifact.entities.0.get(dimension) == Some(*value))
            });
            errors.extend(missing.map(|value| ResolveError::MissingRequiredValue {
                product: rule.product.clone(),
                rule_index,
                context: context.clone(),
                dimension: dimension.clone(),
                value: value.clone(),
            }));
        }
        gaps.extend(errors.into_iter().map(|error| CoverageGap {
            error,
            sources: members.clone(),
        }));
    }
    gaps
}

/// Index valid products by name; the first of several same-named ones wins.
fn index_products<'a>(
    products: &'a [ProductDef],
    invocations: &[Invocation],
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) -> BTreeMap<&'a str, &'a ProductDef> {
    let mut indexed = BTreeMap::new();
    for product in products {
        let site = DefinitionSubject::Product(product.name.clone());
        if let Err(error) = check_product(product) {
            // A step's output takes its input's dimensions, so a fault in an
            // input that already failed would only be reported again.
            let inherited = invocations
                .iter()
                .filter(|invocation| invocation.outputs.contains(&product.name))
                .flat_map(|invocation| &invocation.inputs)
                .any(|input| poisoned.contains(input.product_name()));
            if !inherited {
                errors.push((site, error));
            }
            poisoned.insert(product.name.clone());
        } else if indexed.contains_key(product.name.as_str()) {
            errors.push((
                site,
                ResolveError::InvalidDefinition {
                    subject: DefinitionSubject::Product(product.name.clone()),
                    detail: format!("duplicate product name `{}`", product.name),
                },
            ));
        } else {
            indexed.insert(product.name.as_str(), product);
        }
    }
    indexed
}

fn check_product(product: &ProductDef) -> Result<(), ResolveError> {
    if product.name.is_empty() || !product.artifact_type.is_valid() {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Product(product.name.clone()),
            detail: "product names and artifact types must not be empty".to_owned(),
        });
    }
    if product.artifact_type.has_variables() {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Product(product.name.clone()),
            detail: format!(
                "product `{}` must use a concrete or Unknown type, not an operation variable",
                product.name
            ),
        });
    }
    let dimensions: BTreeSet<_> = product.dimensions.iter().collect();
    if dimensions.len() != product.dimensions.len()
        || product.dimensions.iter().any(String::is_empty)
    {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Product(product.name.clone()),
            detail: format!(
                "product `{}` has duplicate or empty dimensions",
                product.name
            ),
        });
    }
    Ok(())
}

/// Index valid operations by name; the first of several same-named ones wins.
fn index_operations<'a>(
    operations: &'a [OperationDef],
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) -> BTreeMap<&'a str, &'a OperationDef> {
    let mut indexed = BTreeMap::new();
    for operation in operations {
        let site = DefinitionSubject::Operation(operation.name.clone());
        if let Err(error) = check_operation(operation) {
            errors.push((site, error));
            poisoned.insert(operation.name.clone());
        } else if indexed.contains_key(operation.name.as_str()) {
            errors.push((
                site,
                ResolveError::InvalidDefinition {
                    subject: DefinitionSubject::Operation(operation.name.clone()),
                    detail: format!("duplicate operation name `{}`", operation.name),
                },
            ));
        } else {
            indexed.insert(operation.name.as_str(), operation);
        }
    }
    indexed
}

fn check_operation(operation: &OperationDef) -> Result<(), ResolveError> {
    let invalid = |detail: String| ResolveError::InvalidDefinition {
        subject: DefinitionSubject::Operation(operation.name.clone()),
        detail,
    };
    if operation.name.is_empty()
        || operation.outputs.is_empty()
        || operation
            .outputs
            .iter()
            .any(|port| port.name.is_empty() || !port.artifact_type.is_valid())
    {
        return Err(invalid(
            "operation names and output types must not be empty".to_owned(),
        ));
    }
    let mut names: BTreeSet<_> = operation.inputs.iter().map(|port| &port.name).collect();
    if names.len() != operation.inputs.len()
        || operation
            .inputs
            .iter()
            .any(|port| port.name.is_empty() || !port.artifact_type.is_valid())
    {
        return Err(invalid(format!(
            "operation `{}` has invalid input ports",
            operation.name
        )));
    }
    for port in &operation.outputs {
        if !names.insert(&port.name) {
            return Err(invalid(format!(
                "operation `{}` has more than one port named `{}`",
                operation.name, port.name
            )));
        }
    }
    let many = operation
        .inputs
        .iter()
        .filter(|port| port.cardinality == Cardinality::Many)
        .count();
    if many > 1 {
        return Err(invalid(format!(
            "operation `{}` has more than one many input; a job groups one collection",
            operation.name
        )));
    }
    if operation.minimum_collection == Some(0) {
        return Err(invalid(format!(
            "operation `{}` needs `@ min(count)` of at least 1",
            operation.name
        )));
    }
    Ok(())
}

/// Each stage is declared once and holds only steps, and stages must not
/// depend on each other in a cycle. A step outside every stage passes the
/// stages it reads from on to the steps that read from it.
fn check_stages(
    pipeline: &Pipeline,
    producers: &BTreeMap<String, usize>,
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
) {
    let stage_error = |name: &str, detail: String| {
        let subject = DefinitionSubject::Stage(name.to_owned());
        (
            subject.clone(),
            ResolveError::InvalidDefinition { subject, detail },
        )
    };
    let mut declared = BTreeSet::new();
    for stage in &pipeline.stages {
        if !declared.insert(stage.name.as_str()) {
            errors.push(stage_error(
                &stage.name,
                format!("duplicate stage `{}`", stage.name),
            ));
        }
    }
    for invocation in &pipeline.invocations {
        let Some(stage) = invocation.stage.as_deref() else {
            continue;
        };
        if !declared.contains(stage) {
            let subject = DefinitionSubject::Invocation(invocation.output_product().to_owned());
            errors.push((
                subject.clone(),
                ResolveError::InvalidDefinition {
                    subject,
                    detail: format!(
                        "step for `{}` belongs to undeclared stage `{stage}`",
                        invocation.output_product()
                    ),
                },
            ));
        }
    }

    // For each stage, the sibling stages it reads from, with one product it
    // makes and the product it reads for each. Nested stages are compared
    // with their siblings only: the stages that split off where two stages'
    // names part. A step written in an outer stage itself, or outside every
    // stage, passes on what it reads.
    let mut upstream: BTreeMap<String, BTreeMap<String, (&str, &str)>> = BTreeMap::new();
    for invocation in &pipeline.invocations {
        let Some(stage) = invocation.stage.as_deref() else {
            continue;
        };
        let consumer: Vec<_> = stage.split('/').collect();
        let mut pending: Vec<&str> = invocation
            .inputs
            .iter()
            .map(InputBinding::product_name)
            .collect();
        let mut visited = BTreeSet::new();
        while let Some(product) = pending.pop() {
            if !visited.insert(product) {
                continue;
            }
            let Some(producer) = producers
                .get(product)
                .map(|&index| &pipeline.invocations[index])
            else {
                continue;
            };
            let made: Vec<_> = producer
                .stage
                .as_deref()
                .map_or_else(Vec::new, |name| name.split('/').collect());
            let shared = consumer
                .iter()
                .zip(&made)
                .take_while(|(left, right)| left == right)
                .count();
            if shared == made.len() && shared < consumer.len() {
                // Made in a stage around this one, or outside every stage.
                pending.extend(producer.inputs.iter().map(InputBinding::product_name));
            } else if shared < made.len() && shared < consumer.len() {
                upstream
                    .entry(consumer[..=shared].join("/"))
                    .or_default()
                    .entry(made[..=shared].join("/"))
                    .or_insert((invocation.output_product(), product));
            }
            // Otherwise the producer is in this stage or one nested in it,
            // and records what it reads itself.
        }
    }

    let mut reported = BTreeSet::new();
    for stage in &pipeline.stages {
        let start = stage.name.as_str();
        if reported.contains(start) {
            continue;
        }
        let Some(cycle) = stage_cycle(&upstream, start) else {
            continue;
        };
        let steps: Vec<_> = cycle
            .windows(2)
            .map(|pair| {
                let (made, read) = upstream[pair[0]][pair[1]];
                format!(
                    "`{made}` in `{}` reads `{read}` from `{}`",
                    pair[0], pair[1]
                )
            })
            .collect();
        reported.extend(cycle.iter().map(|stage| stage.to_owned()));
        errors.push(stage_error(
            start,
            format!(
                "stages must not depend on each other in a cycle: {}",
                join_list(&steps)
            ),
        ));
    }
}

/// `a`, `a and b`, or `a, b, and c`.
fn join_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// The stages from `start` back to itself through the stages each reads
/// from, beginning and ending with `start`, if there is such a path.
fn stage_cycle<'a>(
    upstream: &'a BTreeMap<String, BTreeMap<String, (&str, &str)>>,
    start: &'a str,
) -> Option<Vec<&'a str>> {
    let mut previous: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = VecDeque::from([start]);
    while let Some(stage) = queue.pop_front() {
        for next in upstream
            .get(stage)
            .into_iter()
            .flat_map(BTreeMap::keys)
            .map(String::as_str)
        {
            if next == start {
                let mut path = Vec::new();
                let mut at = stage;
                while at != start {
                    path.push(at);
                    at = previous[at];
                }
                path.push(start);
                path.reverse();
                path.push(start);
                return Some(path);
            }
            if !previous.contains_key(next) {
                previous.insert(next, stage);
                queue.push_back(next);
            }
        }
    }
    None
}

fn index_producers(
    invocations: &[Invocation],
    products: &BTreeMap<&str, &ProductDef>,
    operations: &BTreeMap<&str, &OperationDef>,
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) -> BTreeMap<String, usize> {
    let mut producers = BTreeMap::new();
    for (index, invocation) in invocations.iter().enumerate() {
        let site = DefinitionSubject::Invocation(invocation.output_product().to_owned());
        if poisoned.contains(&invocation.operation) {
            poisoned.extend(invocation.outputs.iter().cloned());
            continue;
        }
        let known = invocation
            .outputs
            .iter()
            .try_for_each(|output| find_product(products, output).map(|_| ()))
            .and_then(|()| find_operation(operations, &invocation.operation));
        if let Err(error) = known {
            if !invocation
                .outputs
                .iter()
                .any(|output| poisoned.contains(output))
            {
                errors.push((site, error));
            }
            poisoned.extend(invocation.outputs.iter().cloned());
            continue;
        }
        for output in &invocation.outputs {
            if producers.contains_key(output) {
                errors.push((
                    site.clone(),
                    ResolveError::InvalidDefinition {
                        subject: site.clone(),
                        detail: format!(
                            "product `{output}` has more than one producing invocation"
                        ),
                    },
                ));
            } else {
                producers.insert(output.clone(), index);
            }
        }
    }
    producers
}

fn find_product<'a>(
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

fn find_operation<'a>(
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

/// How one step's inputs shape its jobs, worked out from the pipeline alone.
#[derive(Clone, Debug)]
struct StepShape {
    /// The binding that drives the step: one job per artifact of a preserve
    /// step's driver, or per group of an aggregate step's many input.
    driver: usize,
    /// The dimensions every output takes: each job's context.
    context: Vec<String>,
    /// For each binding, the dimensions it is matched to the context on.
    joins: Vec<Vec<String>>,
}

/// A binding's product dimensions, less any `where` pins, and whether it
/// takes many artifacts.
pub(crate) struct BoundInput<'a> {
    pub(crate) binding: &'a InputBinding,
    pub(crate) dimensions: Vec<String>,
    pub(crate) many: bool,
}

impl BoundInput<'_> {
    /// The dimensions this input is matched on when it does not drive.
    fn joins(&self) -> Vec<String> {
        self.binding
            .same
            .clone()
            .unwrap_or_else(|| self.dimensions.clone())
    }
}

/// The driving input and the dimensions of a step's outputs, or `None`
/// when no single-artifact input has every dimension the others match on.
/// The driver is the input with the most dimensions, so the order of an
/// operation's ports never changes which jobs exist.
pub(crate) fn step_context(inputs: &[BoundInput<'_>]) -> Option<(usize, Vec<String>)> {
    if let Some(index) = inputs.iter().position(|input| input.many) {
        let vary = inputs[index].binding.vary.as_deref();
        let context = inputs[index]
            .dimensions
            .iter()
            .filter(|dimension| Some(dimension.as_str()) != vary)
            .cloned()
            .collect();
        return Some((index, context));
    }
    let covers = |index: usize| {
        let dimensions = dimension_set(&inputs[index].dimensions);
        inputs
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .all(|(_, input)| dimension_set(&input.joins()).is_subset(&dimensions))
    };
    let index = (0..inputs.len())
        .filter(|index| inputs[*index].binding.same.is_none() && covers(*index))
        .min_by_key(|index| (Reverse(inputs[*index].dimensions.len()), *index))?;
    Some((index, inputs[index].dimensions.clone()))
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
    let output_product = invocation.output_product();
    let mut substitutions = Substitutions::default();
    for (port, binding) in operation.inputs.iter().zip(&invocation.inputs) {
        let product = find_product(products, binding.product_name())?;
        unify_port(
            &mut substitutions,
            operation,
            output_product,
            &port.name,
            &product.name,
            &port.artifact_type,
            inferred_types
                .get(&product.name)
                .unwrap_or(&product.artifact_type),
        )?;
    }
    for (port, output) in operation.outputs.iter().zip(&outputs) {
        unify_port(
            &mut substitutions,
            operation,
            output_product,
            &port.name,
            &output.name,
            &port.artifact_type,
            &output.artifact_type,
        )?;
    }
    let shape = step_shape(invocation, operation, products, &outputs)?;
    let inferred = operation
        .outputs
        .iter()
        .zip(&outputs)
        .map(|(port, output)| {
            let inferred = substitutions
                .substitute(&port.artifact_type)
                .erase_variables();
            if inferred == TypeExpr::Unknown {
                output.artifact_type.clone()
            } else {
                inferred
            }
        })
        .collect();
    Ok((inferred, shape))
}

/// Check each binding's selectors against its port and product, then find
/// the step's driver and the dimensions of its outputs.
fn step_shape(
    invocation: &Invocation,
    operation: &OperationDef,
    products: &BTreeMap<&str, &ProductDef>,
    outputs: &[&ProductDef],
) -> Result<StepShape, ResolveError> {
    let mut inputs = Vec::new();
    for (port, binding) in operation.inputs.iter().zip(&invocation.inputs) {
        let product = find_product(products, binding.product_name())?;
        check_selectors(operation, port, binding, product)?;
        inputs.push(BoundInput {
            binding,
            dimensions: binding.free_dimensions(&product.dimensions),
            many: port.cardinality == Cardinality::Many,
        });
    }
    let many_ports = inputs.iter().filter(|input| input.many).count();
    match &operation.shape_rule {
        ShapeRule::Preserve => {
            if operation.aggregated_dimension.is_some() {
                return Err(unsupported(
                    operation,
                    "preserve operation cannot declare a dropped dimension",
                ));
            }
            if operation.minimum_collection.is_some() {
                return Err(unsupported(
                    operation,
                    "`@ min(count)` requires a many input",
                ));
            }
            if inputs.is_empty() || many_ports != 0 {
                return Err(unsupported(
                    operation,
                    "preserve requires one or more single-artifact ports",
                ));
            }
        }
        ShapeRule::Aggregate => {
            if many_ports != 1 {
                return Err(unsupported(
                    operation,
                    "aggregation requires exactly one many-artifact port",
                ));
            }
        }
    }
    let Some((driver, context)) = step_context(&inputs) else {
        return Err(no_driver(operation, &inputs));
    };
    if let Some(dimension) = &inputs[driver].binding.vary {
        if let Some(declared) = &operation.aggregated_dimension {
            if declared != dimension {
                return Err(unsupported(
                    operation,
                    format!("declares drop({declared}) but invocation uses vary({dimension})"),
                ));
            }
        }
        // Every other input is matched once per group.
        let grouped = dimension_set(&context);
        for (index, input) in inputs.iter().enumerate() {
            let extra = missing_from(&input.joins(), &grouped);
            if index != driver && !extra.is_empty() {
                return Err(unsupported(
                    operation,
                    format!(
                        "input `{}` has dimensions absent from the groups of `{}` @ vary({dimension}): {}; aggregate them first, pin them with `@ where(...)`, or match on fewer with `@ same(...)`",
                        input.binding.product,
                        inputs[driver].binding.product,
                        extra.join(", ")
                    ),
                ));
            }
        }
    }
    let expected = dimension_set(&context);
    for output in outputs {
        if dimension_set(&output.dimensions) != expected {
            let detail = match &inputs[driver].binding.vary {
                Some(dimension) => format!(
                    "output `{}` must have input dimensions minus `{dimension}`",
                    output.name
                ),
                None => format!(
                    "output `{}` must have the dimensions of driving product `{}`: [{}]",
                    output.name,
                    inputs[driver].binding.product,
                    context.join(", ")
                ),
            };
            return Err(unsupported(operation, detail));
        }
    }
    let joins = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            if index == driver {
                input.dimensions.clone()
            } else {
                input.joins()
            }
        })
        .collect();
    Ok(StepShape {
        driver,
        context,
        joins,
    })
}

fn check_selectors(
    operation: &OperationDef,
    port: &InputPort,
    binding: &InputBinding,
    product: &ProductDef,
) -> Result<(), ResolveError> {
    let selector_error = |detail: String| unsupported(operation, detail);
    for dimension in binding.pinned.keys() {
        if !product.dimensions.contains(dimension) {
            return Err(selector_error(format!(
                "`@ where({dimension}=...)`: product `{}` has no dimension `{dimension}`",
                product.name
            )));
        }
    }
    let free = binding.free_dimensions(&product.dimensions);
    match port.cardinality {
        Cardinality::Many => {
            let Some(dimension) = &binding.vary else {
                return Err(selector_error(format!(
                    "many input `{}` requires an explicit vary(dimension) binding",
                    port.name
                )));
            };
            if !free.contains(dimension) {
                return Err(ResolveError::InvalidAggregationDimension {
                    product: product.name.clone(),
                    dimension: dimension.clone(),
                });
            }
            if binding.same.is_some() {
                return Err(selector_error(format!(
                    "`@ same(...)` applies to single-artifact inputs, not many input `{}`",
                    port.name
                )));
            }
        }
        Cardinality::One => {
            if binding.vary.is_some() {
                return Err(selector_error(format!(
                    "`@ vary(...)` applies to many inputs; input `{}` takes one artifact",
                    port.name
                )));
            }
            if let Some(same) = &binding.same {
                if dimension_set(same).len() != same.len() {
                    return Err(selector_error(format!(
                        "`@ same(...)` repeats a dimension of `{}`",
                        product.name
                    )));
                }
                if let Some(dimension) = same.iter().find(|dimension| !free.contains(*dimension)) {
                    return Err(selector_error(format!(
                        "`@ same({dimension})`: product `{}` has no unpinned dimension `{dimension}`",
                        product.name
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Explain why no input can drive a preserve step: the input with the most
/// dimensions lacks some that another input is matched on.
fn no_driver(operation: &OperationDef, inputs: &[BoundInput<'_>]) -> ResolveError {
    let Some(driver) = (0..inputs.len())
        .filter(|index| inputs[*index].binding.same.is_none())
        .min_by_key(|index| (Reverse(inputs[*index].dimensions.len()), *index))
    else {
        return unsupported(
            operation,
            "every input uses `@ same(...)`, so none can drive the step",
        );
    };
    let dimensions = dimension_set(&inputs[driver].dimensions);
    let (input, extra) = inputs
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != driver)
        .map(|(_, input)| (input, missing_from(&input.joins(), &dimensions)))
        .find(|(_, extra)| !extra.is_empty())
        .expect("a driver is missing only when some input has other dimensions");
    unsupported(
        operation,
        format!(
            "input `{}` has dimensions absent from driving product `{}`: {}; aggregate them first, pin them with `@ where(...)`, or match on fewer with `@ same(...)`",
            input.binding.product,
            inputs[driver].binding.product,
            extra.join(", ")
        ),
    )
}

fn missing_from(dimensions: &[String], available: &BTreeSet<String>) -> Vec<String> {
    dimensions
        .iter()
        .filter(|dimension| !available.contains(*dimension))
        .cloned()
        .collect()
}

fn unify_port(
    substitutions: &mut Substitutions,
    operation: &OperationDef,
    output_product: &str,
    port: &str,
    product: &str,
    expected: &TypeExpr,
    actual: &TypeExpr,
) -> Result<(), ResolveError> {
    substitutions
        .unify(expected, actual)
        .map(|_| ())
        .map_err(|error| match error {
            TypeUnifyError::VariableConflict {
                variable,
                previous,
                required,
            } => ResolveError::TypeVariableConflict {
                operation: operation.name.clone(),
                output_product: output_product.to_owned(),
                port: port.to_owned(),
                product: product.to_owned(),
                conflict: Box::new(TypeConflict {
                    variable,
                    previous,
                    required,
                }),
            },
            _ => ResolveError::TypeMismatch {
                operation: operation.name.clone(),
                output_product: output_product.to_owned(),
                port: port.to_owned(),
                product: product.to_owned(),
                expected: Box::new(expected.clone()),
                found: Box::new(actual.clone()),
            },
        })
}

fn dimension_set(dimensions: &[String]) -> BTreeSet<String> {
    dimensions.iter().cloned().collect()
}

fn invocation_order(
    invocations: &[Invocation],
    producers: &BTreeMap<String, usize>,
) -> Result<Vec<usize>, ResolveError> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Unvisited,
        InProgress,
        Done,
    }

    fn visit(
        index: usize,
        invocations: &[Invocation],
        producers: &BTreeMap<String, usize>,
        states: &mut [State],
        stack: &mut Vec<usize>,
        order: &mut Vec<usize>,
    ) -> Result<(), ResolveError> {
        match states[index] {
            State::Done => return Ok(()),
            State::InProgress => {
                let start = stack.iter().position(|value| *value == index).unwrap_or(0);
                let mut products: Vec<_> = stack[start..]
                    .iter()
                    .map(|value| invocations[*value].output_product().to_owned())
                    .collect();
                products.push(invocations[index].output_product().to_owned());
                return Err(ResolveError::Cycle { products });
            }
            State::Unvisited => {}
        }
        states[index] = State::InProgress;
        stack.push(index);
        for input in &invocations[index].inputs {
            if let Some(producer) = producers.get(input.product_name()) {
                visit(*producer, invocations, producers, states, stack, order)?;
            }
        }
        stack.pop();
        states[index] = State::Done;
        order.push(index);
        Ok(())
    }

    let mut states = vec![State::Unvisited; invocations.len()];
    let mut stack = Vec::new();
    let mut order = Vec::new();
    for index in 0..invocations.len() {
        visit(
            index,
            invocations,
            producers,
            &mut states,
            &mut stack,
            &mut order,
        )?;
    }
    Ok(order)
}

/// `inputs` is complete only when `gaps` is empty.
struct Expansion {
    inputs: Vec<Vec<ArtifactInstance>>,
    outputs: Vec<ArtifactInstance>,
    gaps: Vec<Gap>,
}

/// Enumerate one step's jobs: one per driving artifact, or per group of the
/// many input, with every other input matched to that job's context.
fn expand_step(
    invocation: &Invocation,
    operation: &OperationDef,
    shape: &StepShape,
    outputs: &[(&ProductDef, &TypeExpr)],
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
    incomplete: &BTreeSet<ArtifactKey>,
) -> Vec<Expansion> {
    let candidates: Vec<Vec<&ArtifactInstance>> = invocation
        .inputs
        .iter()
        .map(|binding| {
            family(artifacts, binding.product_name())
                .iter()
                .filter(|artifact| {
                    binding
                        .pinned
                        .iter()
                        .all(|(dimension, value)| artifact.entities.0.get(dimension) == Some(value))
                })
                .collect()
        })
        .collect();
    // Families are sorted, so groups keep the order of their first artifact
    // and each collection is in natural entity order.
    let mut groups: Vec<(EntityBinding, Vec<ArtifactInstance>)> = Vec::new();
    let mut group_index: BTreeMap<EntityBinding, usize> = BTreeMap::new();
    for artifact in &candidates[shape.driver] {
        let context = artifact
            .entities
            .project(&shape.context)
            .expect("an artifact binds every dimension of its product");
        let index = *group_index.entry(context.clone()).or_insert_with(|| {
            groups.push((context, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push((*artifact).clone());
    }
    let driving_port = &operation.inputs[shape.driver];
    let mut expansions = Vec::new();
    for (context, driven) in groups {
        let mut gaps = Vec::new();
        if let Some(minimum) = operation.minimum_collection {
            if driven.len() < minimum {
                gaps.push(Gap::Unmatched(ResolveError::CollectionTooSmall {
                    operation: operation.name.clone(),
                    output_product: invocation.output_product().to_owned(),
                    port: driving_port.name.clone(),
                    context: context.clone(),
                    minimum,
                    found: driven.len(),
                }));
            }
        }
        let mut inputs = Vec::new();
        for (index, port) in operation.inputs.iter().enumerate() {
            let bound = if index == shape.driver {
                driven.clone()
            } else {
                let joins = &shape.joins[index];
                let matches: Vec<_> = candidates[index]
                    .iter()
                    .filter(|candidate| {
                        joins.iter().all(|dimension| {
                            candidate.entities.0.get(dimension) == context.0.get(dimension)
                        })
                    })
                    .collect();
                let product = &invocation.inputs[index].product;
                match matches.as_slice() {
                    [artifact] => vec![(**artifact).clone()],
                    [] => {
                        gaps.push(Gap::Unmatched(ResolveError::MissingInput {
                            operation: operation.name.clone(),
                            output_product: invocation.output_product().to_owned(),
                            port: port.name.clone(),
                            product: product.clone(),
                            context: Box::new(context.clone()),
                        }));
                        continue;
                    }
                    _ => {
                        gaps.push(Gap::Unmatched(ResolveError::AmbiguousInput {
                            operation: operation.name.clone(),
                            output_product: invocation.output_product().to_owned(),
                            port: port.name.clone(),
                            product: product.clone(),
                            context: Box::new(context.clone()),
                        }));
                        continue;
                    }
                }
            };
            gaps.extend(
                bound
                    .iter()
                    .filter(|artifact| incomplete.contains(&artifact.key()))
                    .map(|artifact| Gap::Blocked {
                        port: port.name.clone(),
                        artifact: artifact.clone(),
                    }),
            );
            inputs.push(bound);
        }
        let outputs = outputs
            .iter()
            .map(|(product, artifact_type)| ArtifactInstance {
                product: product.name.clone(),
                artifact_type: (*artifact_type).clone(),
                entities: context.clone(),
            })
            .collect();
        expansions.push(Expansion {
            inputs,
            outputs,
            gaps,
        });
    }
    expansions
}

fn make_job(
    id: usize,
    operation: &OperationDef,
    stage: Option<String>,
    inputs: Vec<Vec<ArtifactInstance>>,
    outputs: Vec<ArtifactInstance>,
    artifact_producers: &BTreeMap<ArtifactKey, usize>,
) -> Job {
    let dependencies: BTreeSet<_> = inputs
        .iter()
        .flatten()
        .filter_map(|input| artifact_producers.get(&input.key()).copied())
        .collect();
    Job {
        id,
        operation: operation.name.clone(),
        inputs,
        outputs,
        dependencies: dependencies.into_iter().collect(),
        stage,
    }
}

fn family<'a>(
    artifacts: &'a BTreeMap<String, Vec<ArtifactInstance>>,
    product: &str,
) -> &'a [ArtifactInstance] {
    artifacts.get(product).map_or(&[], Vec::as_slice)
}

fn unsupported(operation: &OperationDef, detail: impl Into<String>) -> ResolveError {
    ResolveError::UnsupportedShapeRelationship {
        operation: operation.name.clone(),
        detail: detail.into(),
    }
}
