//! Checks of declarations that need no inventory: products, operations,
//! the steps that produce each product, stages, and the order steps run in.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{Cardinality, InputBinding, Invocation, OperationDef, Pipeline, ProductDef};

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
    if operation
        .inputs
        .iter()
        .any(|port| port.name.is_empty() || !port.artifact_type.is_valid())
    {
        return Err(invalid(format!(
            "operation `{}` has an input port with an empty name or type",
            operation.name
        )));
    }
    let mut names = BTreeSet::new();
    let ports = operation.inputs.iter().map(|port| &port.name);
    for name in ports.chain(operation.outputs.iter().map(|port| &port.name)) {
        if !names.insert(name) {
            return Err(invalid(format!(
                "operation `{}` has more than one port named `{name}`",
                operation.name
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
                    .or_insert_with(|| (invocation.output_product(), product));
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
        reported.extend(cycle.iter().map(std::borrow::ToOwned::to_owned));
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
            // Keep in step with `Artifacts` in `model.rs`, which keeps one
            // type per product because only one step makes each.
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

/// Steps that read each other's outputs in a loop, so none can go first.
pub(super) struct Cycle {
    /// The step the loop was found at, by its first output.
    pub(super) start: String,
    /// The first output of each step in the loop, from `start` back to it.
    pub(super) products: Vec<String>,
}

impl Cycle {
    /// The error, and the step to report it at.
    pub(super) fn into_error(self) -> (DefinitionSubject, ResolveError) {
        let Self { start, products } = self;
        (
            DefinitionSubject::Invocation(start),
            ResolveError::Cycle { products },
        )
    }
}

/// Invocation indices with every producer before its consumers, or the
/// first cycle that makes that impossible.
pub(super) fn invocation_order(
    invocations: &[Invocation],
    producers: &BTreeMap<String, usize>,
) -> Result<Vec<usize>, Cycle> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Unvisited,
        InProgress,
        Done,
    }

    let mut states = vec![State::Unvisited; invocations.len()];
    let mut order = Vec::new();
    for root in 0..invocations.len() {
        if states[root] != State::Unvisited {
            continue;
        }
        // Depth first, with an explicit stack so a long chain of steps
        // cannot overflow the call stack: each frame is a step being
        // visited and how many of its inputs have been followed.
        states[root] = State::InProgress;
        let mut frames = vec![(root, 0)];
        while let Some((index, next)) = frames.last_mut() {
            let index = *index;
            let Some(input) = invocations[index].inputs.get(*next) else {
                frames.pop();
                states[index] = State::Done;
                order.push(index);
                continue;
            };
            *next += 1;
            let Some(&producer) = producers.get(input.product_name()) else {
                continue;
            };
            match states[producer] {
                State::Done => {}
                State::InProgress => {
                    let start = frames
                        .iter()
                        .position(|&(value, _)| value == producer)
                        .unwrap_or(0);
                    let start_product = invocations[producer].output_product().to_owned();
                    let mut products: Vec<_> = frames[start..]
                        .iter()
                        .map(|&(value, _)| invocations[value].output_product().to_owned())
                        .collect();
                    products.push(start_product.clone());
                    return Err(Cycle {
                        start: start_product,
                        products,
                    });
                }
                State::Unvisited => {
                    states[producer] = State::InProgress;
                    frames.push((producer, 0));
                }
            }
        }
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::invocation_order;
    use crate::model::{InputBinding, Invocation};

    /// A chain of `steps`, each reading the product of the one before and
    /// written in reverse, so ordering must walk the whole chain at once.
    fn chain(steps: usize) -> (Vec<Invocation>, BTreeMap<String, usize>) {
        let invocations: Vec<_> = (1..=steps)
            .rev()
            .map(|step| {
                let input = InputBinding::product(format!("p{}", step - 1));
                Invocation::new("step", vec![input], format!("p{step}"))
            })
            .collect();
        let producers = invocations
            .iter()
            .enumerate()
            .map(|(index, invocation)| (invocation.output_product().to_owned(), index))
            .collect();
        (invocations, producers)
    }

    #[test]
    fn a_long_chain_is_ordered_without_deep_recursion() {
        // Test threads have small stacks; recursing once per step would
        // overflow long before this.
        let (invocations, producers) = chain(100_000);
        let Ok(order) = invocation_order(&invocations, &producers) else {
            panic!("a chain has no cycle");
        };
        assert_eq!(order.first(), Some(&(invocations.len() - 1)));
        assert_eq!(order.last(), Some(&0));
    }
}
