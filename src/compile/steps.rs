//! How a step's inputs shape its jobs, worked out from the pipeline alone:
//! each binding's selectors, the input that drives the step, and the
//! dimensions each other input is matched on.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::ResolveError;
use crate::model::{
    Cardinality, InputBinding, InputPort, Invocation, OperationDef, ProductDef, ShapeRule,
};
use crate::shape::{broadcast_dimensions, dimension_set, step_context, step_driver, BoundInput};

use super::{find_product, unsupported};

/// How one step's inputs shape its jobs, worked out from the pipeline alone.
#[derive(Clone, Debug)]
pub(crate) struct StepShape {
    /// The binding that drives the step: one job per artifact of a preserve
    /// step's driver, or per group of an aggregate step's many input.
    pub(crate) driver: usize,
    /// The dimensions the driver groups its artifacts by. Each job's context,
    /// which every output takes, adds any an input broadcasts with
    /// `@ each(...)`.
    pub(crate) groups: Vec<String>,
    /// For each binding, the dimensions it is matched to the context on.
    pub(crate) joins: Vec<Vec<String>>,
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
