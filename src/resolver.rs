use std::collections::{BTreeMap, BTreeSet};

use crate::error::ResolveError;
use crate::model::{
    ArtifactInstance, Cardinality, CountRequirement, EntityBinding, InputBinding, Invocation, Job,
    OperationDef, Pipeline, ProductDef, ResolvedDag, ShapeRule, SourceInventory,
};

type ArtifactKey = (String, EntityBinding);

pub fn resolve(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ResolvedDag, ResolveError> {
    let products = index_products(&pipeline.products)?;
    let operations = index_operations(&pipeline.operations)?;
    let producers = index_producers(&pipeline.invocations, &products, &operations)?;

    // Check every invocation's local contract before expanding any concrete job.
    for invocation in &pipeline.invocations {
        validate_invocation(invocation, &products, &operations)?;
    }

    let order = invocation_order(&pipeline.invocations, &producers)?;
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
    validate_coverage(pipeline, inventory, &products, &producers, &artifacts)?;

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
        let jobs = match &operation.shape_rule {
            ShapeRule::Preserve => expand_preserve(
                invocation,
                operation,
                output_def,
                &artifacts,
                &artifact_producers,
                dag.jobs.len(),
            )?,
            ShapeRule::Aggregate => {
                let InputBinding::Vary { dimension, .. } = &invocation.inputs[0] else {
                    unreachable!("validated aggregation binding")
                };
                expand_aggregate(
                    invocation,
                    operation,
                    output_def,
                    dimension,
                    &artifacts,
                    &artifact_producers,
                    dag.jobs.len(),
                )?
            }
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

fn validate_coverage(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    products: &BTreeMap<&str, &ProductDef>,
    producers: &BTreeMap<String, usize>,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
) -> Result<(), ResolveError> {
    for rule in &pipeline.constraints {
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

        let groups: BTreeSet<_> = inventory
            .contexts
            .iter()
            .chain(inventory.artifacts.iter().map(|record| &record.entities))
            .filter_map(|binding| project(binding, &rule.group_by))
            .collect();
        for context in groups {
            let found = family(artifacts, &rule.product)
                .iter()
                .filter(|artifact| {
                    project(&artifact.entities, &rule.group_by) == Some(context.clone())
                })
                .count();
            let valid = match rule.count {
                CountRequirement::Exactly(expected) => found == expected,
                CountRequirement::AtLeast(minimum) => found >= minimum,
            };
            if !valid {
                return Err(ResolveError::CoverageViolation {
                    product: rule.product.clone(),
                    context,
                    expected: rule.count.clone(),
                    found,
                });
            }
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

fn index_products(products: &[ProductDef]) -> Result<BTreeMap<&str, &ProductDef>, ResolveError> {
    let mut indexed = BTreeMap::new();
    for product in products {
        if product.name.is_empty() || product.artifact_type.0.is_empty() {
            return Err(ResolveError::InvalidDefinition {
                detail: "product names and artifact types must not be empty".to_owned(),
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
        if indexed.insert(product.name.as_str(), product).is_some() {
            return Err(ResolveError::InvalidDefinition {
                detail: format!("duplicate product name `{}`", product.name),
            });
        }
    }
    Ok(indexed)
}

fn index_operations(
    operations: &[OperationDef],
) -> Result<BTreeMap<&str, &OperationDef>, ResolveError> {
    let mut indexed = BTreeMap::new();
    for operation in operations {
        if operation.name.is_empty() || operation.output_type.0.is_empty() {
            return Err(ResolveError::InvalidDefinition {
                detail: "operation names and output types must not be empty".to_owned(),
            });
        }
        let ports: BTreeSet<_> = operation.inputs.iter().map(|port| &port.name).collect();
        if ports.len() != operation.inputs.len()
            || operation
                .inputs
                .iter()
                .any(|port| port.name.is_empty() || port.artifact_type.0.is_empty())
        {
            return Err(ResolveError::InvalidDefinition {
                detail: format!("operation `{}` has invalid input ports", operation.name),
            });
        }
        if indexed.insert(operation.name.as_str(), operation).is_some() {
            return Err(ResolveError::InvalidDefinition {
                detail: format!("duplicate operation name `{}`", operation.name),
            });
        }
    }
    Ok(indexed)
}

fn index_producers(
    invocations: &[Invocation],
    products: &BTreeMap<&str, &ProductDef>,
    operations: &BTreeMap<&str, &OperationDef>,
) -> Result<BTreeMap<String, usize>, ResolveError> {
    let mut producers = BTreeMap::new();
    for (index, invocation) in invocations.iter().enumerate() {
        find_product(products, &invocation.output_product)?;
        find_operation(operations, &invocation.operation)?;
        if producers
            .insert(invocation.output_product.clone(), index)
            .is_some()
        {
            return Err(ResolveError::InvalidDefinition {
                detail: format!(
                    "product `{}` has more than one producing invocation",
                    invocation.output_product
                ),
            });
        }
    }
    Ok(producers)
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
) -> Result<(), ResolveError> {
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
    if output.artifact_type != operation.output_type {
        return Err(ResolveError::TypeMismatch {
            operation: operation.name.clone(),
            port: "output".to_owned(),
            product: output.name.clone(),
            expected: operation.output_type.clone(),
            found: output.artifact_type.clone(),
        });
    }
    for (port, binding) in operation.inputs.iter().zip(&invocation.inputs) {
        let product = find_product(products, binding.product_name())?;
        if product.artifact_type != port.artifact_type {
            return Err(ResolveError::TypeMismatch {
                operation: operation.name.clone(),
                port: port.name.clone(),
                product: product.name.clone(),
                expected: port.artifact_type.clone(),
                found: product.artifact_type.clone(),
            });
        }
    }
    match &operation.shape_rule {
        ShapeRule::Preserve => {
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
    Ok(())
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
                        port: port.name.clone(),
                        context: driving_artifact.entities.clone(),
                        candidates,
                    })
                }
            }
        }
        let output = ArtifactInstance {
            product: output_def.name.clone(),
            artifact_type: output_def.artifact_type.clone(),
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
    dimension: &str,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
    artifact_producers: &BTreeMap<ArtifactKey, usize>,
    existing_jobs: usize,
) -> Result<Vec<Job>, ResolveError> {
    let mut groups: BTreeMap<EntityBinding, Vec<ArtifactInstance>> = BTreeMap::new();
    for artifact in family(artifacts, invocation.inputs[0].product_name()) {
        groups
            .entry(artifact.entities.without(dimension))
            .or_default()
            .push(artifact.clone());
    }
    let mut jobs = Vec::new();
    for (entities, mut inputs) in groups {
        inputs.sort();
        let output = ArtifactInstance {
            product: output_def.name.clone(),
            artifact_type: output_def.artifact_type.clone(),
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
    Ok(jobs)
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
