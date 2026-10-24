//! Checks of declarations that need no inventory: products, operations,
//! the steps that produce each product, stages, and the order steps run in.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashSet;

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{Cardinality, Invocation, OperationDef, Pipeline, ProductDef};

use super::{find_operation, find_product};

/// Index valid products by name; the first of several same-named ones wins.
pub(super) fn index_products<'a>(
    products: &'a [ProductDef],
    invocations: &[Invocation],
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
    duplicated: &mut BTreeSet<String>,
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
            duplicated.insert(product.name.clone());
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

/// Each stage is declared once, and each step's stage is declared.
pub(super) fn check_stages(
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

fn stage_error(name: &str, detail: String) -> (DefinitionSubject, ResolveError) {
    let subject = DefinitionSubject::Stage(name.to_owned());
    (
        subject.clone(),
        ResolveError::InvalidDefinition { subject, detail },
    )
}

pub(super) fn index_producers(
    invocations: &[Invocation],
    products: &BTreeMap<&str, &ProductDef>,
    operations: &BTreeMap<&str, &OperationDef>,
    duplicated: &BTreeSet<String>,
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
            // Keep in step with `Artifacts` in `model/artifacts.rs`, which keeps one
            // type per product because only one step makes each.
            if producers.contains_key(output) {
                // Neither step is the product's, so no type or shape of
                // either reaches the steps that read it.
                poisoned.insert(output.clone());
                // Two steps that name a product each declare it, which was
                // reported as a duplicate at the second step's line.
                if !duplicated.contains(output) {
                    errors.push((
                        site.clone(),
                        ResolveError::InvalidDefinition {
                            subject: site.clone(),
                            detail: format!(
                                "product `{output}` has more than one producing invocation"
                            ),
                        },
                    ));
                }
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
    /// Every step that reads its way back to itself through `start`: the
    /// whole strongly connected component, which may hold more than one loop.
    pub(super) steps: Vec<usize>,
}

impl Cycle {
    /// The error, and the step to report it at.
    pub(super) fn into_error(self) -> (DefinitionSubject, ResolveError) {
        let Self {
            start, products, ..
        } = self;
        (
            DefinitionSubject::Invocation(start),
            ResolveError::Cycle { products },
        )
    }
}

/// Steps in dependency order, and the loops that keep some from having one.
pub(super) struct Order {
    /// Every step, each after every step it reads from that is not in a loop
    /// with it. A step in a cycle is here too, so the caller can skip it and
    /// every step that reads from it.
    pub(super) steps: Vec<usize>,
    /// One cycle per group of steps that read each other, in the order found.
    pub(super) cycles: Vec<Cycle>,
}

/// Invocation indices with every producer before its consumers, and each
/// group of steps that read each other in a loop, reported once.
///
/// This is Tarjan's strongly connected components, walking from each step to
/// the producers of its inputs. A component is finished only after every
/// component it reads from, so the order they finish in is a dependency order.
pub(super) fn invocation_order(
    invocations: &[Invocation],
    producers: &BTreeMap<String, usize>,
) -> Order {
    let producer = |step: usize, port: usize| {
        let input = invocations[step].inputs.get(port)?;
        Some(producers.get(input.product_name()).copied())
    };
    let count = invocations.len();
    let mut walk = Walk {
        reached: vec![None; count],
        lowest: vec![0; count],
        on_stack: vec![false; count],
        stack: Vec::new(),
        next: 0,
    };
    let mut order = Order {
        steps: Vec::with_capacity(count),
        cycles: Vec::new(),
    };
    for root in 0..count {
        if walk.reached[root].is_some() {
            continue;
        }
        // Depth first, with an explicit stack so a long chain of steps
        // cannot overflow the call stack: each frame is a step being
        // visited and how many of its inputs have been followed.
        let mut frames = Vec::new();
        walk.enter(root, &mut frames);
        while let Some(&mut (step, ref mut port)) = frames.last_mut() {
            if let Some(found) = producer(step, *port) {
                *port += 1;
                let Some(input) = found else {
                    continue;
                };
                match walk.reached[input] {
                    None => walk.enter(input, &mut frames),
                    Some(at) if walk.on_stack[input] => {
                        walk.lowest[step] = walk.lowest[step].min(at);
                    }
                    Some(_) => {}
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                walk.lowest[parent] = walk.lowest[parent].min(walk.lowest[step]);
            }
            if Some(walk.lowest[step]) != walk.reached[step] {
                continue;
            }
            let at = walk
                .stack
                .iter()
                .rposition(|&member| member == step)
                .expect("a step being finished is on the stack");
            let component = walk.stack.split_off(at);
            for &member in &component {
                walk.on_stack[member] = false;
            }
            let reads_itself = (0..invocations[step].inputs.len())
                .any(|port| producer(step, port) == Some(Some(step)));
            if component.len() > 1 || reads_itself {
                order
                    .cycles
                    .push(cycle_through(invocations, producers, step, &component));
            }
            order.steps.extend(component);
        }
    }
    order
}

/// What the walk in `invocation_order` knows of each step.
struct Walk {
    /// When each step was first reached.
    reached: Vec<Option<usize>>,
    /// The earliest step still on `stack` that each step reaches back to.
    lowest: Vec<usize>,
    on_stack: Vec<bool>,
    /// Steps reached whose component is not finished yet.
    stack: Vec<usize>,
    next: usize,
}

impl Walk {
    fn enter(&mut self, step: usize, frames: &mut Vec<(usize, usize)>) {
        self.reached[step] = Some(self.next);
        self.lowest[step] = self.next;
        self.next += 1;
        self.stack.push(step);
        self.on_stack[step] = true;
        frames.push((step, 0));
    }
}

/// A loop from `start` back to itself through the steps of `component`.
fn cycle_through(
    invocations: &[Invocation],
    producers: &BTreeMap<String, usize>,
    start: usize,
    component: &[usize],
) -> Cycle {
    let members: FxHashSet<usize> = component.iter().copied().collect();
    let mut seen = FxHashSet::default();
    seen.insert(start);
    let mut frames = vec![(start, 0)];
    loop {
        let (step, port) = frames
            .last_mut()
            .expect("every step of a component reads its way back to the first");
        let Some(input) = invocations[*step].inputs.get(*port) else {
            frames.pop();
            continue;
        };
        *port += 1;
        let Some(&input) = producers.get(input.product_name()) else {
            continue;
        };
        if input == start {
            break;
        }
        if members.contains(&input) && seen.insert(input) {
            frames.push((input, 0));
        }
    }
    let name = |step: usize| invocations[step].output_product().to_owned();
    let mut products: Vec<_> = frames.iter().map(|&(step, _)| name(step)).collect();
    products.push(name(start));
    let mut steps = component.to_vec();
    steps.sort_unstable();
    Cycle {
        start: name(start),
        products,
        steps,
    }
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
        let order = invocation_order(&invocations, &producers);
        assert!(order.cycles.is_empty(), "a chain has no cycle");
        assert_eq!(order.steps.first(), Some(&(invocations.len() - 1)));
        assert_eq!(order.steps.last(), Some(&0));
    }
}
