use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::model::ResolveError;
use crate::model::{
    Cardinality, CollectionRule, InputBinding, InputPort, Invocation, OperationDef, ProductDef,
    ShapeRule,
};
use crate::shape::{broadcast_dimensions, dimension_set, step_context, step_driver, BoundInput};

use super::{find_product, unsupported};

#[derive(Clone, Debug)]
pub(crate) struct StepShape {
    pub(crate) driver: usize,
    pub(crate) groups: Vec<String>,
    pub(crate) joins: Vec<Vec<String>>,
    pub(crate) selections: Vec<(usize, Vec<String>)>,
    pub(crate) varied: Vec<String>,
    pub(crate) collection: Option<CollectionRule>,
}

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
    check_paired_collections(operation, &inputs, driver)?;
    check_vary(operation, &inputs, driver, &context)?;
    check_output_dimensions(operation, &inputs, driver, &groups, &context, outputs)?;
    let joins = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            if index == driver {
                input.dimensions.clone()
            } else if input.many {
                groups.clone()
            } else {
                input.joins()
            }
        })
        .collect::<Vec<_>>();
    let varied = if inputs[driver].many {
        inputs[driver]
            .dimensions
            .iter()
            .filter(|dimension| !groups.contains(dimension))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let collection = collection_rule(operation, &inputs);
    check_beside_inputs(operation, &inputs, &joins, &context)?;
    let selections = inputs
        .into_iter()
        .enumerate()
        .filter(|(_, input)| input.binding.selection.is_some())
        .map(|(index, input)| (index, input.dimensions))
        .collect();
    Ok(StepShape {
        driver,
        groups,
        joins,
        selections,
        varied,
        collection,
    })
}

fn collection_rule(operation: &OperationDef, inputs: &[BoundInput<'_>]) -> Option<CollectionRule> {
    let ports: Vec<usize> = inputs
        .iter()
        .enumerate()
        .filter_map(|(port, input)| input.many.then_some(port))
        .collect();
    let first = *ports.first()?;
    Some(CollectionRule {
        member: inputs[first].binding.vary.clone(),
        ports,
        minimum: operation.minimum_collection.unwrap_or(1).max(1),
    })
}

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
                    "`@min(count)` requires a many input",
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
            if many_ports == 0 {
                return Err(unsupported(
                    operation,
                    "aggregation requires a many-artifact port",
                ));
            }
        }
    }
    Ok(())
}

fn check_paired_collections(
    operation: &OperationDef,
    inputs: &[BoundInput<'_>],
    driver: usize,
) -> Result<(), ResolveError> {
    if !inputs[driver].many {
        return Ok(());
    }
    let driving = inputs[driver].binding;
    for (index, input) in inputs.iter().enumerate() {
        if !input.many || index == driver {
            continue;
        }
        let binding = input.binding;
        let same = dimension_set(&input.dimensions) == dimension_set(&inputs[driver].dimensions)
            && dimension_set(&binding.vary) == dimension_set(&driving.vary);
        if !same || binding.same.is_some() || !binding.each.is_empty() {
            return Err(unsupported(
                operation,
                format!(
                    "many inputs `{}` and `{}` must vary over the same dimensions, so that each job pairs their artifacts: [{}] and [{}]; `@same(...)` and `@each(...)` do not go on them",
                    driving.product,
                    binding.product,
                    inputs[driver].dimensions.join(", "),
                    input.dimensions.join(", "),
                ),
            ));
        }
    }
    Ok(())
}

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
        .filter(|(index, input)| *index != driver && !input.many)
        .map(|(_, input)| (input, missing_from(&input.joins(), &grouped)))
        .find(|(_, extra)| !extra.is_empty());
    match unmatched {
        Some((input, extra)) => {
            let suggestion = extra
                .iter()
                .filter(|dimension| !driving.vary.contains(dimension))
                .map(|dimension| format!("`{} @each({dimension})`", input.binding.product))
                .collect::<Vec<_>>();
            let hint = if suggestion.is_empty() {
                String::new()
            } else {
                format!(
                    "; to broadcast these dimensions, try {}",
                    suggestion.join(" with ")
                )
            };
            Err(unsupported(
                operation,
                format!(
                    "input `{}` has dimensions absent from the groups of `{}` @vary({varied}): {}{hint}; aggregate them first, pin them with `@where(...)`, or match on fewer with `@same(...)`",
                    input.binding.product,
                    driving.product,
                    extra.join(", ")
                ),
            ))
        }
        None => Ok(()),
    }
}

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
        ", plus those broadcast with `@each(...)`"
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

fn check_beside_inputs(
    operation: &OperationDef,
    inputs: &[BoundInput<'_>],
    joins: &[Vec<String>],
    context: &[String],
) -> Result<(), ResolveError> {
    let expected = dimension_set(context);
    for port in &operation.outputs {
        let Some(beside) = &port.beside else { continue };
        let Some(index) = operation
            .inputs
            .iter()
            .position(|input| input.name == beside.sibling)
        else {
            continue;
        };
        let input = &inputs[index];
        let product = &input.binding.product;
        let listed = |dimensions: &[String]| format!("[{}]", dimensions.join(", "));
        let problem = if dimension_set(&input.dimensions) != expected {
            format!(
                "`{product}` has dimensions {}, and the step's jobs {}",
                listed(&input.dimensions),
                listed(context)
            )
        } else if dimension_set(&joins[index]) != expected {
            format!(
                "`{product}` is matched on {} only, of the step's {}",
                listed(&joins[index]),
                listed(context)
            )
        } else {
            continue;
        };
        return Err(unsupported(
            operation,
            format!(
                "output `{}` is written beside input `{}`, so each job must read its own `{}` file, but {problem}; write it beside an input with the step's dimensions, or give it a path of its own",
                port.name, beside.sibling, beside.sibling
            ),
        ));
    }
    Ok(())
}

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
                    "`@each({dimension})` on `{product}` would restore the dimension `{}` @vary({dimension}) collects",
                    driving.product
                )
            } else if groups.contains(dimension) {
                format!(
                    "`@each({dimension})` on `{product}`: driving product `{}` already has `{dimension}`, so it is matched without `@each`",
                    driving.product
                )
            } else if let Some(first) = broadcast.insert(dimension, product) {
                format!(
                    "`{first}` and `{product}` both broadcast `{dimension}`; keep `@each({dimension})` on one and the other is matched on it"
                )
            } else {
                continue;
            };
            return Err(unsupported(operation, problem));
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
    let fail = |detail: String| Err(unsupported(operation, detail));
    let name = &product.name;
    if let Some(dimension) = binding
        .pinned
        .keys()
        .find(|dimension| !product.dimensions.contains(*dimension))
    {
        return fail(format!(
            "`@where({dimension}=...)`: product `{name}` has no dimension `{dimension}`"
        ));
    }
    if let Some(selection) = &binding.selection {
        let keyword = selection.keyword();
        let dimension = selection.dimension();
        if port.cardinality == Cardinality::Many {
            return fail(format!(
                "`@{keyword}(...)` applies to single-artifact inputs; input `{}` takes many",
                port.name
            ));
        }
        if !product.dimensions.iter().any(|name| name == dimension) {
            return fail(format!(
                "`@{keyword}({dimension})`: product `{name}` has no dimension `{dimension}`"
            ));
        }
        if binding.pinned.contains_key(dimension) {
            return fail(format!(
                "`@{keyword}({dimension})`: `@where(...)` already pins `{dimension}` on `{name}`"
            ));
        }
    }
    let free = binding.free_dimensions(&product.dimensions);
    let not_free = |selector: &str, dimensions: &[String]| {
        dimensions
            .iter()
            .find(|dimension| !free.contains(*dimension))
            .map(|dimension| {
                if binding.selection.as_ref().is_some_and(|choice| choice.dimension() == dimension) {
                    format!("`@{selector}({dimension})`: selection already removed `{dimension}` from `{name}`")
                } else {
                    format!("`@{selector}({dimension})`: product `{name}` has no unpinned dimension `{dimension}`")
                }
            })
    };
    let port_name = &port.name;
    match port.cardinality {
        Cardinality::Many => {
            if binding.vary.is_empty() {
                return fail(format!(
                    "many input `{port_name}` needs `@vary(dimension)` naming the dimensions it collects, as in `{name} @vary(run)`"
                ));
            }
            if dimension_set(&binding.vary).len() != binding.vary.len() {
                return fail(format!("`@vary(...)` repeats a dimension of `{name}`"));
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
                    "`@{selector}(...)` applies to single-artifact inputs, not many input `{port_name}`"
                ));
            }
        }
        Cardinality::One => {
            if !binding.vary.is_empty() {
                return fail(format!(
                    "`@vary(...)` applies to many inputs; input `{port_name}` takes one artifact"
                ));
            }
            let same = binding.same.as_deref().unwrap_or_default();
            if dimension_set(same).len() != same.len() {
                return fail(format!("`@same(...)` repeats a dimension of `{name}`"));
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

fn no_driver(operation: &OperationDef, inputs: &[BoundInput<'_>]) -> ResolveError {
    let Some(driver) = (0..inputs.len())
        .filter(|index| inputs[*index].can_drive())
        .min_by_key(|index| (Reverse(inputs[*index].dimensions.len()), *index))
    else {
        return unsupported(
            operation,
            "every input uses `@same(...)` or `@each(...)`, so none can drive the step",
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
            "input `{}` has dimensions absent from driving product `{}`: {}; aggregate them first, pin them with `@where(...)`, match on fewer with `@same(...)`, or broadcast them with `@each(...)`",
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
