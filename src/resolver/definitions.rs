//! Checks of declarations that need no inventory: products, operations,
//! the steps that produce each product, stages, and the order steps run in.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{Cardinality, InputBinding, Invocation, OperationDef, Pipeline, ProductDef};
use crate::paths::PathPlaceholder;

use super::{find_operation, find_product};

/// Index valid products by name; the first of several same-named ones wins.
pub(super) fn index_products<'a>(
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
    if let Some(dimension) = product
        .dimensions
        .iter()
        .find(|dimension| PathPlaceholder::reserved(dimension).is_some())
    {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Product(product.name.clone()),
            detail: format!(
                "product `{}` cannot have a dimension named `{dimension}`, which path templates reserve for `{{{dimension}}}`",
                product.name
            ),
        });
    }
    Ok(())
}

/// Index valid operations by name; the first of several same-named ones wins.
pub(super) fn index_operations<'a>(
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
pub(super) fn check_stages(
    pipeline: &Pipeline,
    producers: &BTreeMap<String, usize>,
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
) {
    check_stage_declarations(pipeline, errors);
    let upstream = stage_dependencies(pipeline, producers);
    check_stage_cycles(pipeline, &upstream, errors);
}

fn stage_error(name: &str, detail: String) -> (DefinitionSubject, ResolveError) {
    let subject = DefinitionSubject::Stage(name.to_owned());
    (
        subject.clone(),
        ResolveError::InvalidDefinition { subject, detail },
    )
}

/// Each stage is declared once, and each step's stage is declared.
fn check_stage_declarations(
    pipeline: &Pipeline,
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
) {
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
}

/// For each stage, the sibling stages it reads from, with one product it
/// makes and the product it reads for each.
type StageDependencies<'a> = BTreeMap<String, BTreeMap<String, (&'a str, &'a str)>>;

/// Which stages each stage reads from. Nested stages are compared with their
/// siblings only: the stages that split off where two stages' names part. A
/// step written in an outer stage itself, or outside every stage, passes on
/// what it reads.
fn stage_dependencies<'a>(
    pipeline: &'a Pipeline,
    producers: &BTreeMap<String, usize>,
) -> StageDependencies<'a> {
    let mut upstream: StageDependencies<'a> = BTreeMap::new();
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
    upstream
}

/// Report each cycle of stages that read from each other, once.
fn check_stage_cycles(
    pipeline: &Pipeline,
    upstream: &StageDependencies<'_>,
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
) {
    let mut reported = BTreeSet::new();
    for stage in &pipeline.stages {
        let start = stage.name.as_str();
        if reported.contains(start) {
            continue;
        }
        let Some(cycle) = stage_cycle(upstream, start) else {
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
fn stage_cycle<'a>(upstream: &'a StageDependencies<'_>, start: &'a str) -> Option<Vec<&'a str>> {
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

pub(super) fn index_producers(
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

pub(super) fn invocation_order(
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
