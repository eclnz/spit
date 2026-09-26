use std::collections::{BTreeMap, BTreeSet};

use crate::error::ResolveError;
use crate::model::{
    ArtifactInstance, Cardinality, CountRequirement, CoverageRule, EntityBinding, InputBinding,
    Invocation, Job, OperationDef, Pipeline, ProductDef, ResolvedDag, ShapeRule, SourceInventory,
};
use crate::types::{Substitutions, TypeExpr, TypeUnifyError};

type ArtifactKey = (String, EntityBinding);

/// The part of the pipeline text a check error belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Site {
    Product(String),
    Operation(String),
    /// A step, named by the product it produces.
    Invocation(String),
    /// A coverage rule, by its index in `Pipeline::constraints`.
    Rule(usize),
}

/// A pipeline whose declarations, steps, and rules hold without any inventory.
struct CheckedPipeline<'a> {
    products: BTreeMap<&'a str, &'a ProductDef>,
    operations: BTreeMap<&'a str, &'a OperationDef>,
    producers: BTreeMap<String, usize>,
    /// Invocation indices with every producer before its consumers.
    order: Vec<usize>,
    /// The statically inferred output type of each checked step.
    inferred_types: BTreeMap<String, TypeExpr>,
}

/// Every pipeline error, plus the names that failed or depend on a failure.
pub(crate) struct PipelineCheck<'a> {
    pipeline: CheckedPipeline<'a>,
    pub errors: Vec<(Site, ResolveError)>,
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
    let products = index_products(&pipeline.products, &mut errors, &mut poisoned);
    let operations = index_operations(&pipeline.operations, &mut errors, &mut poisoned);
    let producers = index_producers(
        &pipeline.invocations,
        &products,
        &operations,
        &mut errors,
        &mut poisoned,
    );
    let order = match invocation_order(&pipeline.invocations, &producers) {
        Ok(order) => order,
        Err(error) => {
            let ResolveError::Cycle { products } = &error else {
                unreachable!("ordering only reports cycles")
            };
            errors.push((Site::Invocation(products[0].clone()), error));
            (0..pipeline.invocations.len()).collect()
        }
    };
    // Inferred intermediate types are also part of the reusable pipeline
    // contract. Check them in dependency order so a type error does not depend
    // on whether an inventory happens to contain concrete source artifacts.
    // A step that fails, or uses something that failed, quiets its consumers.
    let mut inferred_types = BTreeMap::new();
    for &index in &order {
        let invocation = &pipeline.invocations[index];
        let depends_on_failure = poisoned.contains(&invocation.operation)
            || poisoned.contains(&invocation.output_product)
            || invocation
                .inputs
                .iter()
                .any(|input| poisoned.contains(input.product_name()));
        if depends_on_failure {
            poisoned.insert(invocation.output_product.clone());
            continue;
        }
        match validate_invocation(invocation, &products, &operations, &inferred_types) {
            Ok(inferred) => {
                inferred_types.insert(invocation.output_product.clone(), inferred);
            }
            Err(error) => {
                errors.push((Site::Invocation(invocation.output_product.clone()), error));
                poisoned.insert(invocation.output_product.clone());
            }
        }
    }
    for (index, rule) in pipeline.constraints.iter().enumerate() {
        if poisoned.contains(&rule.product) {
            continue;
        }
        if let Err(error) = check_coverage_rule(rule, &products, &producers) {
            errors.push((Site::Rule(index), error));
        }
    }
    PipelineCheck {
        pipeline: CheckedPipeline {
            products,
            operations,
            producers,
            order,
            inferred_types,
        },
        errors,
        poisoned,
    }
}

pub fn resolve(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ResolvedDag, ResolveError> {
    let CheckedPipeline {
        products,
        operations,
        producers,
        order,
        inferred_types,
    } = check_pipeline(pipeline)?;
    let mut artifacts: BTreeMap<String, Vec<ArtifactInstance>> = BTreeMap::new();
    let mut artifact_producers: BTreeMap<ArtifactKey, usize> = BTreeMap::new();
    let mut seen: BTreeSet<ArtifactKey> = BTreeSet::new();

    for record in &inventory.artifacts {
        let product = find_product(&products, &record.product)?;
        if producers.contains_key(&record.product) {
            return Err(ResolveError::InvalidDefinition {
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
                detail: format!(
                    "source `{source}` must bind exactly the dimensions of product `{}`: {:?}",
                    product.name, product.dimensions
                ),
            });
        }
        let key = artifact_key(&source);
        if !seen.insert(key) {
            return Err(ResolveError::DuplicateOutputArtifact { artifact: source });
        }
        artifacts
            .entry(source.product.clone())
            .or_default()
            .push(source);
    }
    for family in artifacts.values_mut() {
        family.sort();
    }
    for (rule_index, rule) in pipeline.constraints.iter().enumerate() {
        validate_coverage(rule_index, rule, inventory, &artifacts)?;
    }

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
        let output_def = products[invocation.output_product.as_str()];
        // Every artifact in a family has the same type, so the type inferred
        // statically for the invocation is the type of each job's output.
        let output_type = &inferred_types[&invocation.output_product];
        let jobs = match &operation.shape_rule {
            ShapeRule::Preserve => expand_preserve(
                invocation,
                operation,
                output_def,
                output_type,
                &artifacts,
                &artifact_producers,
                dag.jobs.len(),
            )?,
            ShapeRule::Aggregate => expand_aggregate(
                invocation,
                operation,
                output_def,
                output_type,
                &artifacts,
                &artifact_producers,
                dag.jobs.len(),
            ),
        };

        for job in jobs {
            let key = artifact_key(&job.output);
            if !seen.insert(key.clone()) {
                return Err(ResolveError::DuplicateOutputArtifact {
                    artifact: job.output,
                });
            }
            artifact_producers.insert(key, job.id);
            artifacts
                .entry(job.output.product.clone())
                .or_default()
                .push(job.output.clone());
            dag.jobs.push(job);
        }
        if let Some(family) = artifacts.get_mut(&invocation.output_product) {
            family.sort();
        }
    }
    Ok(dag)
}

fn check_coverage_rule(
    rule: &CoverageRule,
    products: &BTreeMap<&str, &ProductDef>,
    producers: &BTreeMap<String, usize>,
) -> Result<(), ResolveError> {
    let product = find_product(products, &rule.product)?;
    if producers.contains_key(&rule.product) {
        return Err(ResolveError::InvalidDefinition {
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
            detail: format!(
                "coverage rule for `{}` must group by distinct dimensions of that product",
                rule.product
            ),
        });
    }
    Ok(())
}

fn validate_coverage(
    rule_index: usize,
    rule: &CoverageRule,
    inventory: &SourceInventory,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
) -> Result<(), ResolveError> {
    let groups: BTreeSet<_> = inventory
        .contexts
        .iter()
        .chain(inventory.artifacts.iter().map(|record| &record.entities))
        .filter_map(|binding| project(binding, &rule.group_by))
        .collect();
    for context in groups {
        let found = family(artifacts, &rule.product)
            .iter()
            .filter(|artifact| project(&artifact.entities, &rule.group_by) == Some(context.clone()))
            .count();
        let valid = match rule.count {
            CountRequirement::Exactly(expected) => found == expected,
            CountRequirement::AtLeast(minimum) => found >= minimum,
        };
        if !valid {
            return Err(ResolveError::CoverageViolation {
                product: rule.product.clone(),
                rule_index,
                context,
                expected: rule.count.clone(),
                found,
            });
        }
    }
    Ok(())
}

fn project(binding: &EntityBinding, dimensions: &[String]) -> Option<EntityBinding> {
    let values = dimensions
        .iter()
        .map(|dimension| {
            binding
                .0
                .get(dimension)
                .map(|value| (dimension.clone(), value.clone()))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    Some(EntityBinding(values))
}

/// Index valid products by name; the first of several same-named ones wins.
fn index_products<'a>(
    products: &'a [ProductDef],
    errors: &mut Vec<(Site, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) -> BTreeMap<&'a str, &'a ProductDef> {
    let mut indexed = BTreeMap::new();
    for product in products {
        let site = Site::Product(product.name.clone());
        if let Err(error) = check_product(product) {
            errors.push((site, error));
            poisoned.insert(product.name.clone());
        } else if indexed.contains_key(product.name.as_str()) {
            errors.push((
                site,
                ResolveError::InvalidDefinition {
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
            detail: "product names and artifact types must not be empty".to_owned(),
        });
    }
    if product.artifact_type.has_variables() {
        return Err(ResolveError::InvalidDefinition {
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
    errors: &mut Vec<(Site, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) -> BTreeMap<&'a str, &'a OperationDef> {
    let mut indexed = BTreeMap::new();
    for operation in operations {
        let site = Site::Operation(operation.name.clone());
        if let Err(error) = check_operation(operation) {
            errors.push((site, error));
            poisoned.insert(operation.name.clone());
        } else if indexed.contains_key(operation.name.as_str()) {
            errors.push((
                site,
                ResolveError::InvalidDefinition {
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
    if operation.name.is_empty() || !operation.output_type.is_valid() {
        return Err(ResolveError::InvalidDefinition {
            detail: "operation names and output types must not be empty".to_owned(),
        });
    }
    let ports: BTreeSet<_> = operation.inputs.iter().map(|port| &port.name).collect();
    if ports.len() != operation.inputs.len()
        || operation
            .inputs
            .iter()
            .any(|port| port.name.is_empty() || !port.artifact_type.is_valid())
    {
        return Err(ResolveError::InvalidDefinition {
            detail: format!("operation `{}` has invalid input ports", operation.name),
        });
    }
    Ok(())
}

fn index_producers(
    invocations: &[Invocation],
    products: &BTreeMap<&str, &ProductDef>,
    operations: &BTreeMap<&str, &OperationDef>,
    errors: &mut Vec<(Site, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) -> BTreeMap<String, usize> {
    let mut producers = BTreeMap::new();
    for (index, invocation) in invocations.iter().enumerate() {
        let site = Site::Invocation(invocation.output_product.clone());
        if poisoned.contains(&invocation.operation) {
            poisoned.insert(invocation.output_product.clone());
            continue;
        }
        let known = find_product(products, &invocation.output_product)
            .and_then(|_| find_operation(operations, &invocation.operation));
        if let Err(error) = known {
            if !poisoned.contains(&invocation.output_product) {
                errors.push((site, error));
            }
            poisoned.insert(invocation.output_product.clone());
        } else if producers.contains_key(&invocation.output_product) {
            errors.push((
                site,
                ResolveError::InvalidDefinition {
                    detail: format!(
                        "product `{}` has more than one producing invocation",
                        invocation.output_product
                    ),
                },
            ));
        } else {
            producers.insert(invocation.output_product.clone(), index);
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

fn validate_invocation(
    invocation: &Invocation,
    products: &BTreeMap<&str, &ProductDef>,
    operations: &BTreeMap<&str, &OperationDef>,
    inferred_types: &BTreeMap<String, TypeExpr>,
) -> Result<TypeExpr, ResolveError> {
    let operation = find_operation(operations, &invocation.operation)?;
    let output = find_product(products, &invocation.output_product)?;
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
    let mut substitutions = Substitutions::default();
    for (port, binding) in operation.inputs.iter().zip(&invocation.inputs) {
        let product = find_product(products, binding.product_name())?;
        unify_port(
            &mut substitutions,
            operation,
            &invocation.output_product,
            &port.name,
            &product.name,
            &port.artifact_type,
            inferred_types
                .get(&product.name)
                .unwrap_or(&product.artifact_type),
        )?;
    }
    unify_port(
        &mut substitutions,
        operation,
        &invocation.output_product,
        "output",
        &output.name,
        &operation.output_type,
        &output.artifact_type,
    )?;
    match &operation.shape_rule {
        ShapeRule::Preserve => {
            if operation.aggregated_dimension.is_some() {
                return Err(unsupported(
                    operation,
                    "preserve operation cannot declare a dropped dimension",
                ));
            }
            if operation.inputs.is_empty()
                || operation
                    .inputs
                    .iter()
                    .any(|port| port.cardinality != Cardinality::One)
                || invocation
                    .inputs
                    .iter()
                    .any(|binding| !matches!(binding, InputBinding::Product(_)))
            {
                return Err(unsupported(
                    operation,
                    "preserve requires one or more single-artifact ports bound to products",
                ));
            }
            let driver = find_product(products, invocation.inputs[0].product_name())?;
            if dimension_set(&driver.dimensions) != dimension_set(&output.dimensions) {
                return Err(unsupported(
                    operation,
                    format!(
                        "output `{}` must have the dimensions of driving product `{}`",
                        output.name, driver.name
                    ),
                ));
            }
        }
        ShapeRule::Aggregate => {
            if operation.inputs.len() != 1 || operation.inputs[0].cardinality != Cardinality::Many {
                return Err(unsupported(
                    operation,
                    "aggregation requires exactly one many-artifact port",
                ));
            }
            let InputBinding::Vary { product, dimension } = &invocation.inputs[0] else {
                return Err(unsupported(
                    operation,
                    "aggregation requires an explicit vary(dimension) binding",
                ));
            };
            if let Some(declared) = &operation.aggregated_dimension {
                if declared != dimension {
                    return Err(unsupported(
                        operation,
                        format!("declares drop({declared}) but invocation uses vary({dimension})"),
                    ));
                }
            }
            let input = find_product(products, product)?;
            if !input.dimensions.contains(dimension) {
                return Err(ResolveError::InvalidAggregationDimension {
                    product: product.clone(),
                    dimension: dimension.clone(),
                });
            }
            let expected: BTreeSet<_> = input
                .dimensions
                .iter()
                .filter(|value| *value != dimension)
                .cloned()
                .collect();
            if expected != dimension_set(&output.dimensions) {
                return Err(unsupported(
                    operation,
                    format!(
                        "output `{}` must have input dimensions minus `{dimension}`",
                        output.name
                    ),
                ));
            }
        }
    }
    let inferred = substitutions
        .substitute(&operation.output_type)
        .erase_variables();
    if inferred == TypeExpr::Unknown {
        Ok(output.artifact_type.clone())
    } else {
        Ok(inferred)
    }
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
                variable,
                previous: Box::new(previous),
                required: Box::new(required),
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
    fn visit(
        index: usize,
        invocations: &[Invocation],
        producers: &BTreeMap<String, usize>,
        states: &mut [u8],
        stack: &mut Vec<usize>,
        order: &mut Vec<usize>,
    ) -> Result<(), ResolveError> {
        match states[index] {
            2 => return Ok(()),
            1 => {
                let start = stack.iter().position(|value| *value == index).unwrap_or(0);
                let mut products: Vec<_> = stack[start..]
                    .iter()
                    .map(|value| invocations[*value].output_product.clone())
                    .collect();
                products.push(invocations[index].output_product.clone());
                return Err(ResolveError::Cycle { products });
            }
            _ => {}
        }
        states[index] = 1;
        stack.push(index);
        for input in &invocations[index].inputs {
            if let Some(producer) = producers.get(input.product_name()) {
                visit(*producer, invocations, producers, states, stack, order)?;
            }
        }
        stack.pop();
        states[index] = 2;
        order.push(index);
        Ok(())
    }

    let mut states = vec![0; invocations.len()];
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

fn expand_preserve(
    invocation: &Invocation,
    operation: &OperationDef,
    output_def: &ProductDef,
    output_type: &TypeExpr,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
    artifact_producers: &BTreeMap<ArtifactKey, usize>,
    existing_jobs: usize,
) -> Result<Vec<Job>, ResolveError> {
    let driver = family(artifacts, invocation.inputs[0].product_name());
    let mut jobs = Vec::new();
    for driving_artifact in driver {
        let mut inputs = vec![driving_artifact.clone()];
        for (port, binding) in operation.inputs.iter().zip(&invocation.inputs).skip(1) {
            let candidates: Vec<_> = family(artifacts, binding.product_name())
                .iter()
                .filter(|candidate| {
                    candidate
                        .entities
                        .matches_shared(&driving_artifact.entities)
                })
                .cloned()
                .collect();
            match candidates.len() {
                0 => {
                    return Err(ResolveError::MissingInput {
                        operation: operation.name.clone(),
                        output_product: invocation.output_product.clone(),
                        port: port.name.clone(),
                        context: driving_artifact.entities.clone(),
                    })
                }
                1 => {
                    let candidate = &candidates[0];
                    if !candidate
                        .entities
                        .0
                        .keys()
                        .all(|dimension| driving_artifact.entities.0.contains_key(dimension))
                    {
                        return Err(unsupported(
                            operation,
                            format!(
                                "input `{}` has dimensions absent from driving product `{}`",
                                binding.product_name(),
                                driving_artifact.product
                            ),
                        ));
                    }
                    inputs.push(candidate.clone());
                }
                _ => {
                    return Err(ResolveError::AmbiguousInput {
                        operation: operation.name.clone(),
                        output_product: invocation.output_product.clone(),
                        port: port.name.clone(),
                        context: driving_artifact.entities.clone(),
                        candidates,
                    })
                }
            }
        }
        let output = ArtifactInstance {
            product: output_def.name.clone(),
            artifact_type: output_type.clone(),
            entities: driving_artifact.entities.clone(),
        };
        jobs.push(make_job(
            existing_jobs + jobs.len() + 1,
            operation,
            inputs,
            output,
            artifact_producers,
        ));
    }
    Ok(jobs)
}

fn expand_aggregate(
    invocation: &Invocation,
    operation: &OperationDef,
    output_def: &ProductDef,
    output_type: &TypeExpr,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
    artifact_producers: &BTreeMap<ArtifactKey, usize>,
    existing_jobs: usize,
) -> Vec<Job> {
    let InputBinding::Vary { dimension, .. } = &invocation.inputs[0] else {
        unreachable!("validated aggregation binding")
    };
    let mut groups: BTreeMap<EntityBinding, Vec<ArtifactInstance>> = BTreeMap::new();
    for artifact in family(artifacts, invocation.inputs[0].product_name()) {
        groups
            .entry(artifact.entities.without(dimension))
            .or_default()
            .push(artifact.clone());
    }
    let mut jobs = Vec::new();
    for (entities, mut inputs) in groups {
        // Collection arguments follow entity bindings in lexicographic order.
        inputs.sort_by(|left, right| left.entities.cmp(&right.entities));
        let output = ArtifactInstance {
            product: output_def.name.clone(),
            artifact_type: output_type.clone(),
            entities,
        };
        jobs.push(make_job(
            existing_jobs + jobs.len() + 1,
            operation,
            inputs,
            output,
            artifact_producers,
        ));
    }
    jobs
}

fn make_job(
    id: usize,
    operation: &OperationDef,
    inputs: Vec<ArtifactInstance>,
    output: ArtifactInstance,
    artifact_producers: &BTreeMap<ArtifactKey, usize>,
) -> Job {
    let dependencies: BTreeSet<_> = inputs
        .iter()
        .filter_map(|input| artifact_producers.get(&artifact_key(input)).copied())
        .collect();
    Job {
        id,
        operation: operation.name.clone(),
        inputs,
        output,
        dependencies: dependencies.into_iter().collect(),
    }
}

fn family<'a>(
    artifacts: &'a BTreeMap<String, Vec<ArtifactInstance>>,
    product: &str,
) -> &'a [ArtifactInstance] {
    artifacts.get(product).map(Vec::as_slice).unwrap_or(&[])
}

fn artifact_key(artifact: &ArtifactInstance) -> ArtifactKey {
    (artifact.product.clone(), artifact.entities.clone())
}

fn unsupported(operation: &OperationDef, detail: impl Into<String>) -> ResolveError {
    ResolveError::UnsupportedShapeRelationship {
        operation: operation.name.clone(),
        detail: detail.into(),
    }
}
