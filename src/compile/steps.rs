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
    let context = step_context(&inputs, &groups);
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
    let driving = inputs[driver].binding;
    if driving.vary.is_empty() {
        return Ok(());
    }
    let varied = driving.vary.join(", ");
    let grouped = dimension_set(context);
    let unmatched = inputs
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != driver)
        .map(|(_, input)| (input, missing_from(&input.joins(), &grouped)))
        .find(|(_, extra)| !extra.is_empty());
    match unmatched {
        Some((input, extra)) => Err(unsupported(
            operation,
            format!(
                "input `{}` has dimensions absent from the groups of `{}` @ vary({varied}): {}; aggregate them first, pin them with `@ where(...)`, match on fewer with `@ same(...)`, or broadcast them with `@ each(...)`",
                input.binding.product,
                driving.product,
                extra.join(", ")
            ),
        )),
        None => Ok(()),
    }
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
    let Some(output) = outputs
        .iter()
        .find(|output| dimension_set(&output.dimensions) != expected)
    else {
        return Ok(());
    };
    let driving = inputs[driver].binding;
    let less = if driving.vary.is_empty() {
        String::new()
    } else {
        format!(
            " less {}",
            driving
                .vary
                .iter()
                .map(|dimension| format!("`{dimension}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let broadcast = if context.len() > groups.len() {
        ", plus those broadcast with `@ each(...)`"
    } else {
        ""
    };
    Err(unsupported(
        operation,
        format!(
            "output `{}` must have the dimensions of driving product `{}`{less}{broadcast}: [{}]",
            output.name,
            driving.product,
            context.join(", ")
        ),
    ))
}

/// Check that each dimension an input broadcasts is new to the step: the
/// driver does not already have it, and no other input broadcasts it too.
fn check_broadcasts(
    operation: &OperationDef,
    inputs: &[BoundInput<'_>],
    driver: usize,
    groups: &[String],
) -> Result<(), ResolveError> {
    let driving = inputs[driver].binding;
    let mut broadcast: BTreeMap<&str, &str> = BTreeMap::new();
    for input in inputs {
        let product = input.binding.product.as_str();
        for dimension in &input.binding.each {
            let problem = if driving.vary.contains(dimension) {
                format!(
                    "`@ each({dimension})` on `{product}` would restore the dimension `{}` @ vary({dimension}) collects",
                    driving.product
                )
            } else if groups.contains(dimension) {
                format!(
                    "`@ each({dimension})` on `{product}`: driving product `{}` already has `{dimension}`, so it is matched without `@ each`",
                    driving.product
                )
            } else if let Some(first) = broadcast.insert(dimension, product) {
                format!(
                    "`{first}` and `{product}` both broadcast `{dimension}`; keep `@ each({dimension})` on one and the other is matched on it"
                )
            } else {
                continue;
            };
            return Err(unsupported(operation, problem));
        }
    }
    Ok(())
}

/// Check each selector of a binding against its port's cardinality and its
/// product's dimensions.
fn check_selectors(
    operation: &OperationDef,
    port: &InputPort,
    binding: &InputBinding,
    product: &ProductDef,
) -> Result<(), ResolveError> {
    let fail = |detail: String| Err(unsupported(operation, detail));
    let name = &product.name;
    if let Some(dimension) = binding
        .pinned
        .keys()
        .find(|dimension| !product.dimensions.contains(*dimension))
    {
        return fail(format!(
            "`@ where({dimension}=...)`: product `{name}` has no dimension `{dimension}`"
        ));
    }
    let free = binding.free_dimensions(&product.dimensions);
    // The first of `dimensions` a selector names that is pinned or absent.
    let not_free = |selector: &str, dimensions: &[String]| {
        dimensions
            .iter()
            .find(|dimension| !free.contains(*dimension))
            .map(|dimension| {
                format!("`@ {selector}({dimension})`: product `{name}` has no unpinned dimension `{dimension}`")
            })
    };
    let port_name = &port.name;
    match port.cardinality {
        Cardinality::Many => {
            if binding.vary.is_empty() {
                return fail(format!(
                    "many input `{port_name}` needs `@ vary(dimension)` naming the dimensions it collects, as in `{name} @ vary(run)`"
                ));
            }
            if dimension_set(&binding.vary).len() != binding.vary.len() {
                return fail(format!("`@ vary(...)` repeats a dimension of `{name}`"));
            }
            for dimension in &binding.vary {
                if !free.contains(dimension) {
                    return Err(ResolveError::InvalidAggregationDimension {
                        product: name.clone(),
                        dimension: dimension.clone(),
                    });
                }
            }
            let single_only = if binding.same.is_some() {
                Some("same")
            } else if !binding.each.is_empty() {
                Some("each")
            } else {
                None
            };
            if let Some(selector) = single_only {
                return fail(format!(
                    "`@ {selector}(...)` applies to single-artifact inputs, not many input `{port_name}`"
                ));
            }
        }
        Cardinality::One => {
            if !binding.vary.is_empty() {
                return fail(format!(
                    "`@ vary(...)` applies to many inputs; input `{port_name}` takes one artifact"
                ));
            }
            let same = binding.same.as_deref().unwrap_or_default();
            if dimension_set(same).len() != same.len() {
                return fail(format!("`@ same(...)` repeats a dimension of `{name}`"));
            }
            if let Some(problem) =
                not_free("same", same).or_else(|| not_free("each", &binding.each))
            {
                return fail(problem);
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
