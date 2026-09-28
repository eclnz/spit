//! How a step's inputs are matched: checking its bindings and finding the
//! input that drives it, then expanding it into one job per driving
//! artifact or group, with every other input matched to that job.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::ResolveError;
use crate::model::{
    ArtifactInstance, ArtifactKey, Cardinality, EntityBinding, Gap, InputBinding, InputPort,
    Invocation, Job, OperationDef, ProductDef, ShapeRule,
};
use crate::shape::{broadcast_dimensions, dimension_set, step_context, step_driver, BoundInput};
use crate::types::TypeExpr;

use super::{family, find_product, unsupported};

/// How one step's inputs shape its jobs, worked out from the pipeline alone.
#[derive(Clone, Debug)]
pub(super) struct StepShape {
    /// The binding that drives the step: one job per artifact of a preserve
    /// step's driver, or per group of an aggregate step's many input.
    driver: usize,
    /// The dimensions the driver groups its artifacts by. Each job's context,
    /// which every output takes, adds any an input broadcasts with
    /// `@ each(...)`.
    groups: Vec<String>,
    /// For each binding, the dimensions it is matched to the context on.
    joins: Vec<Vec<String>>,
}

/// Check each binding's selectors against its port and product, then find
/// the step's driver and the dimensions of its outputs.
pub(super) fn step_shape(
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
    check_shape_rule(operation, &inputs)?;
    let Some((driver, groups)) = step_driver(&inputs) else {
        return Err(no_driver(operation, &inputs));
    };
    check_broadcasts(operation, &inputs, driver, &groups)?;
    let (_, context) = step_context(&inputs).expect("the step has a driver");
    check_vary(operation, &inputs, driver, &context)?;
    check_output_dimensions(operation, &inputs, driver, &groups, &context, outputs)?;
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
        groups,
        joins,
    })
}

/// A preserve operation takes only single-artifact inputs; an aggregate
/// one takes exactly one many input.
fn check_shape_rule(
    operation: &OperationDef,
    inputs: &[BoundInput<'_>],
) -> Result<(), ResolveError> {
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
    Ok(())
}

/// A driver that varies a dimension groups its artifacts without it, and
/// every other input must be matched once per group.
fn check_vary(
    operation: &OperationDef,
    inputs: &[BoundInput<'_>],
    driver: usize,
    context: &[String],
) -> Result<(), ResolveError> {
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
        let grouped = dimension_set(context);
        for (index, input) in inputs.iter().enumerate() {
            let extra = missing_from(&input.joins(), &grouped);
            if index != driver && !extra.is_empty() {
                return Err(unsupported(
                    operation,
                    format!(
                        "input `{}` has dimensions absent from the groups of `{}` @ vary({dimension}): {}; aggregate them first, pin them with `@ where(...)`, match on fewer with `@ same(...)`, or broadcast them with `@ each(...)`",
                        input.binding.product,
                        inputs[driver].binding.product,
                        extra.join(", ")
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Each output has the dimensions of the step's context: the driver's
/// groups, plus any broadcast.
fn check_output_dimensions(
    operation: &OperationDef,
    inputs: &[BoundInput<'_>],
    driver: usize,
    groups: &[String],
    context: &[String],
    outputs: &[&ProductDef],
) -> Result<(), ResolveError> {
    let expected = dimension_set(context);
    for output in outputs {
        if dimension_set(&output.dimensions) != expected {
            let detail = match &inputs[driver].binding.vary {
                Some(dimension) if context.len() > groups.len() => format!(
                    "output `{}` must have input dimensions minus `{dimension}`, plus those broadcast with `@ each(...)`: [{}]",
                    output.name,
                    context.join(", ")
                ),
                Some(dimension) => format!(
                    "output `{}` must have input dimensions minus `{dimension}`",
                    output.name
                ),
                None if context.len() > groups.len() => format!(
                    "output `{}` must have the dimensions of driving product `{}` and those broadcast with `@ each(...)`: [{}]",
                    output.name,
                    inputs[driver].binding.product,
                    context.join(", ")
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
    Ok(())
}

/// Check that each dimension an input broadcasts is new to the step: the
/// driver does not already have it, and no other input broadcasts it too.
fn check_broadcasts(
    operation: &OperationDef,
    inputs: &[BoundInput<'_>],
    driver: usize,
    groups: &[String],
) -> Result<(), ResolveError> {
    let mut broadcast: BTreeMap<&str, &str> = BTreeMap::new();
    for input in inputs {
        let product = input.binding.product.as_str();
        for dimension in &input.binding.each {
            if let Some(varied) = inputs[driver]
                .binding
                .vary
                .as_ref()
                .filter(|varied| *varied == dimension)
            {
                return Err(unsupported(
                    operation,
                    format!(
                        "`@ each({dimension})` on `{product}` would restore the dimension `{}` @ vary({varied}) collects",
                        inputs[driver].binding.product
                    ),
                ));
            }
            if groups.contains(dimension) {
                return Err(unsupported(
                    operation,
                    format!(
                        "`@ each({dimension})` on `{product}`: driving product `{}` already has `{dimension}`, so it is matched without `@ each`",
                        inputs[driver].binding.product
                    ),
                ));
            }
            if let Some(first) = broadcast.insert(dimension, product) {
                return Err(unsupported(
                    operation,
                    format!(
                        "`{first}` and `{product}` both broadcast `{dimension}`; keep `@ each({dimension})` on one and the other is matched on it"
                    ),
                ));
            }
        }
    }
    Ok(())
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
            if !binding.each.is_empty() {
                return Err(selector_error(format!(
                    "`@ each(...)` applies to single-artifact inputs, not many input `{}`",
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
            if let Some(dimension) = binding
                .each
                .iter()
                .find(|dimension| !free.contains(*dimension))
            {
                return Err(selector_error(format!(
                    "`@ each({dimension})`: product `{}` has no unpinned dimension `{dimension}`",
                    product.name
                )));
            }
        }
    }
    Ok(())
}

/// Explain why no input can drive a preserve step: the input with the most
/// dimensions lacks some that another input is matched on.
fn no_driver(operation: &OperationDef, inputs: &[BoundInput<'_>]) -> ResolveError {
    let Some(driver) = (0..inputs.len())
        .filter(|index| inputs[*index].can_drive())
        .min_by_key(|index| (Reverse(inputs[*index].dimensions.len()), *index))
    else {
        return unsupported(
            operation,
            "every input uses `@ same(...)` or `@ each(...)`, so none can drive the step",
        );
    };
    let mut dimensions = dimension_set(&inputs[driver].dimensions);
    dimensions.extend(broadcast_dimensions(inputs));
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
            "input `{}` has dimensions absent from driving product `{}`: {}; aggregate them first, pin them with `@ where(...)`, match on fewer with `@ same(...)`, or broadcast them with `@ each(...)`",
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

/// `inputs` is complete only when `gaps` is empty.
pub(super) struct Expansion {
    pub(super) inputs: Vec<Vec<ArtifactInstance>>,
    pub(super) outputs: Vec<ArtifactInstance>,
    pub(super) gaps: Vec<Gap>,
}

/// Enumerate one step's jobs: one per driving artifact, or per group of the
/// many input, with every other input matched to that job's context.
pub(super) fn expand_step(
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
    let contexts = broadcast_contexts(invocation, &candidates);
    let jobs = driver_groups(&candidates[shape.driver], shape)
        .into_iter()
        .flat_map(|(group, driven)| {
            contexts.iter().map(move |values| {
                let mut context = group.clone();
                context.0.extend(values.0.clone());
                (context, driven.clone())
            })
        });
    let mut expansions = Vec::new();
    for (context, driven) in jobs {
        let mut gaps = Vec::new();
        if let Some(minimum) = operation.minimum_collection {
            if driven.len() < minimum {
                gaps.push(Gap::Unmatched(ResolveError::CollectionTooSmall {
                    operation: operation.name.clone(),
                    output_product: invocation.output_product().to_owned(),
                    port: operation.inputs[shape.driver].name.clone(),
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
                match match_input(
                    invocation,
                    operation,
                    index,
                    &shape.joins[index],
                    &candidates[index],
                    &context,
                ) {
                    Ok(artifact) => vec![artifact],
                    Err(gap) => {
                        gaps.push(gap);
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

/// The driver's artifacts grouped by the step's groups: one artifact per
/// group for a preserve step, or a collection per group for an aggregate
/// one. Families are sorted, so groups keep the order of their first
/// artifact and each collection is in natural entity order.
fn driver_groups(
    candidates: &[&ArtifactInstance],
    shape: &StepShape,
) -> Vec<(EntityBinding, Vec<ArtifactInstance>)> {
    let mut groups: Vec<(EntityBinding, Vec<ArtifactInstance>)> = Vec::new();
    let mut group_index: BTreeMap<EntityBinding, usize> = BTreeMap::new();
    for artifact in candidates {
        let context = artifact
            .entities
            .project(&shape.groups)
            .expect("an artifact binds every dimension of its product");
        let index = *group_index.entry(context.clone()).or_insert_with(|| {
            groups.push((context, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push((*artifact).clone());
    }
    groups
}

/// The one candidate for input `index` that agrees with a job's context on
/// every dimension it joins on, or the gap left when none or several do.
fn match_input(
    invocation: &Invocation,
    operation: &OperationDef,
    index: usize,
    joins: &[String],
    candidates: &[&ArtifactInstance],
    context: &EntityBinding,
) -> Result<ArtifactInstance, Gap> {
    let matches: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            joins
                .iter()
                .all(|dimension| candidate.entities.0.get(dimension) == context.0.get(dimension))
        })
        .collect();
    let [artifact] = matches.as_slice() else {
        let (operation, output_product, port, product, context) = (
            operation.name.clone(),
            invocation.output_product().to_owned(),
            operation.inputs[index].name.clone(),
            invocation.inputs[index].product.clone(),
            Box::new(context.clone()),
        );
        return Err(Gap::Unmatched(if matches.is_empty() {
            ResolveError::MissingInput {
                operation,
                output_product,
                port,
                product,
                context,
            }
        } else {
            ResolveError::AmbiguousInput {
                operation,
                output_product,
                port,
                product,
                context,
            }
        }));
    };
    Ok((**artifact).clone())
}

/// Every combination of the values the inputs broadcast with `@ each(...)`:
/// for each such input, the values present in its product, in natural order.
/// A step without broadcasts has one empty combination.
fn broadcast_contexts(
    invocation: &Invocation,
    candidates: &[Vec<&ArtifactInstance>],
) -> Vec<EntityBinding> {
    let mut contexts = vec![EntityBinding::default()];
    for (binding, candidates) in invocation.inputs.iter().zip(candidates) {
        if binding.each.is_empty() {
            continue;
        }
        let mut values: Vec<EntityBinding> = Vec::new();
        for candidate in candidates {
            let value = candidate
                .entities
                .project(&binding.each)
                .expect("an artifact binds every dimension of its product");
            if !values.contains(&value) {
                values.push(value);
            }
        }
        contexts = contexts
            .iter()
            .flat_map(|context| {
                values.iter().map(move |value| {
                    let mut combined = context.clone();
                    combined.0.extend(value.0.clone());
                    combined
                })
            })
            .collect();
    }
    contexts
}

pub(super) fn make_job(
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
