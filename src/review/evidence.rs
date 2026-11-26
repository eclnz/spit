use std::collections::VecDeque;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::{PipelineIndex, Port};
use crate::parser::SourceMap;
use crate::template::CommandError;

use super::ReviewIndex;

pub(super) fn dependency_error(
    index: &PipelineIndex<'_>,
    lines: &SourceMap,
) -> Option<CommandError> {
    let pipeline = index.pipeline;
    let reviews = ReviewIndex::new(index);
    let steps = &pipeline.invocations;
    let producers: FxHashMap<_, _> = steps
        .iter()
        .enumerate()
        .flat_map(|(step, invocation)| {
            invocation
                .outputs
                .iter()
                .map(move |name| (name.as_str(), step))
        })
        .collect();
    let mut review_nodes = FxHashMap::default();
    let mut review_keys = Vec::new();
    let mut prerequisites = vec![Vec::new(); steps.len()];
    for product in &pipeline.products {
        let product = product.name.as_str();
        for &name in reviews.of(product) {
            review_nodes.insert((product, name), prerequisites.len());
            review_keys.push((product, name));
            prerequisites.push(producers.get(product).copied().into_iter().collect());
        }
    }

    let mut exemptions: FxHashMap<_, FxHashSet<_>> = FxHashMap::default();
    for invocation in steps {
        for used in &invocation.reviews {
            let (Port::Output(port), Some(evidence)) = (used.port, used.evidence.as_deref()) else {
                continue;
            };
            let Some(candidate) = invocation.outputs.get(port) else {
                continue;
            };
            let Some((&node, &producer)) = review_nodes
                .get(&(candidate.as_str(), used.name.as_str()))
                .zip(producers.get(evidence))
            else {
                continue;
            };
            prerequisites[node].push(producer);
            let ancestors = exemptions.entry(node).or_default();
            let mut pending = vec![producer];
            while let Some(step) = pending.pop() {
                if ancestors.insert(step) {
                    pending.extend(
                        steps[step]
                            .inputs
                            .iter()
                            .filter_map(|input| producers.get(input.product_name()).copied()),
                    );
                }
            }
        }
    }
    for (step, invocation) in steps.iter().enumerate() {
        for input in &invocation.inputs {
            let product = input.product_name();
            prerequisites[step].extend(producers.get(product).copied());
            for &review in reviews.of(product) {
                let gate = review_nodes[&(product, review)];
                if !exemptions
                    .get(&gate)
                    .is_some_and(|ancestors| ancestors.contains(&step))
                {
                    prerequisites[step].push(gate);
                }
            }
        }
    }

    let cycle = find_cycle(&prerequisites)?;
    let (product, review) = cycle.iter().find_map(|node| {
        node.checked_sub(steps.len())
            .map(|review| review_keys[review])
    })?;
    let chain = cycle
        .iter()
        .map(|&node| {
            if node < steps.len() {
                format!("`{}`", steps[node].output_product())
            } else {
                let (product, review) = review_keys[node - steps.len()];
                format!("review `{review}` of `{product}`")
            }
        })
        .collect::<Vec<_>>()
        .join(" needs ");
    let place = lines.products.get(product).cloned();
    Some(
        CommandError::new(format!("review dependency cycle: {chain}"))
            .at(place)
            .focus(review),
    )
}

fn find_cycle(prerequisites: &[Vec<usize>]) -> Option<Vec<usize>> {
    let mut counts = Vec::with_capacity(prerequisites.len());
    let mut readers = vec![Vec::new(); prerequisites.len()];
    let mut ready = VecDeque::new();
    for (node, needs) in prerequisites.iter().enumerate() {
        counts.push(needs.len());
        if needs.is_empty() {
            ready.push_back(node);
        }
        for &dependency in needs {
            readers[dependency].push(node);
        }
    }
    while let Some(node) = ready.pop_front() {
        for &reader in &readers[node] {
            counts[reader] -= 1;
            if counts[reader] == 0 {
                ready.push_back(reader);
            }
        }
    }
    let mut node = counts.iter().position(|&remaining| remaining != 0)?;
    let mut path = Vec::new();
    let mut seen = FxHashMap::default();
    loop {
        if let Some(start) = seen.insert(node, path.len()) {
            path.push(node);
            return Some(path.split_off(start));
        }
        path.push(node);
        node = *prerequisites[node]
            .iter()
            .find(|&&dependency| counts[dependency] != 0)
            .expect("an unsorted node has an unsorted prerequisite");
    }
}
