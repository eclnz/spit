//! Expanding a checked step into jobs: one per driving artifact or group,
//! with every other input matched to that job.

use std::collections::{BTreeMap, BTreeSet};

use crate::compile::StepShape;
use crate::error::{PortSite, ResolveError};
use crate::model::{
    ArtifactInstance, ArtifactKey, EntityBinding, Gap, Invocation, Job, OperationDef, ProductDef,
};
use crate::types::TypeExpr;

use super::family;

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
                    binding.pinned.iter().all(|(dimension, value)| {
                        artifact.entities.get(dimension) == Some(value.as_str())
                    })
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
                context.extend(values);
                (context, driven.clone())
            })
        });
    let mut expansions = Vec::new();
    for (context, driven) in jobs {
        let mut gaps = Vec::new();
        if let Some(minimum) = operation.minimum_collection {
            if driven.len() < minimum {
                gaps.push(Gap::Unmatched(ResolveError::CollectionTooSmall {
                    site: port_site(invocation, operation, shape.driver),
                    context: Box::new(context.clone()),
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
                .all(|dimension| candidate.entities.get(dimension) == context.get(dimension))
        })
        .collect();
    let [artifact] = matches.as_slice() else {
        let site = port_site(invocation, operation, index);
        let context = Box::new(context.clone());
        return Err(Gap::Unmatched(if matches.is_empty() {
            ResolveError::MissingInput { site, context }
        } else {
            ResolveError::AmbiguousInput { site, context }
        }));
    };
    Ok((**artifact).clone())
}

/// Where input `index` of a step is bound.
fn port_site(invocation: &Invocation, operation: &OperationDef, index: usize) -> PortSite {
    PortSite {
        operation: operation.name.clone(),
        output_product: invocation.output_product().to_owned(),
        port: operation.inputs[index].name.clone(),
        product: invocation.inputs[index].product.clone(),
    }
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
                    combined.extend(value);
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
