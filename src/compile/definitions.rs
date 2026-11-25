use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashSet;

use crate::model::{DefinitionSubject, ResolveError};
use crate::model::{Invocation, OperationDef, Pipeline, ProductDef};

use super::{find_operation, find_product};

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
    if operation.outputs.iter().any(|output| output.view) {
        let [output] = operation.outputs.as_slice() else {
            return Err(invalid(
                "a directory view needs exactly one output".to_owned(),
            ));
        };
        if !output.folder
            || output.seed.is_some()
            || output.beside.is_some()
            || operation.inputs.is_empty()
            || operation
                .inputs
                .iter()
                .any(|input| input.cardinality != crate::model::Cardinality::One)
        {
            return Err(invalid(
                "a directory view needs a folder output and one or more single-file inputs"
                    .to_owned(),
            ));
        }
        if !output.checks.is_empty()
            || operation
                .inputs
                .iter()
                .any(|input| !input.checks.is_empty())
        {
            return Err(invalid(
                "attach checks to a directory view’s member producers or consumers".to_owned(),
            ));
        }
    }
    for output in &operation.outputs {
        let Some(seed) = &output.seed else { continue };
        if !output.folder || output.beside.is_some() {
            return Err(invalid(format!(
                "output `{}` must be a folder to use `from {seed}`",
                output.name
            )));
        }
        if !operation
            .inputs
            .iter()
            .any(|input| input.name == *seed && input.cardinality == crate::model::Cardinality::One)
        {
            return Err(invalid(format!(
                "`from {seed}` on output `{}` needs the name of a single-artifact input port",
                output.name
            )));
        }
    }
    if operation.minimum_collection == Some(0) {
        return Err(invalid(format!(
            "operation `{}` needs `@min(count)` of at least 1",
            operation.name
        )));
    }
    Ok(())
}

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
            if producers.contains_key(output) {
                poisoned.insert(output.clone());
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

pub(super) struct Cycle {
    pub(super) start: String,
    pub(super) products: Vec<String>,
    pub(super) steps: Vec<usize>,
}

impl Cycle {
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

pub(super) struct Order {
    pub(super) steps: Vec<usize>,
    pub(super) cycles: Vec<Cycle>,
}

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

struct Walk {
    reached: Vec<Option<usize>>,
    lowest: Vec<usize>,
    on_stack: Vec<bool>,
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
        let (invocations, producers) = chain(100_000);
        let order = invocation_order(&invocations, &producers);
        assert!(order.cycles.is_empty(), "a chain has no cycle");
        assert_eq!(order.steps.first(), Some(&(invocations.len() - 1)));
        assert_eq!(order.steps.last(), Some(&0));
    }
}
