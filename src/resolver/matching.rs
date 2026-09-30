//! Expanding a checked step into jobs: one per driving artifact or group,
//! with every other input matched to that job.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compile::{CompiledStep, StepShape};
use crate::error::{PortSite, ResolveError};
use crate::model::{
    ArtifactInstance, ArtifactMap, ArtifactSet, EntityBinding, Gap, Invocation, Job, OperationDef,
};

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
    step: &CompiledStep<'_>,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
    incomplete: &ArtifactSet,
) -> Vec<Expansion> {
    let (invocation, shape) = (step.invocation, &step.shape);
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
    let indexes: Vec<JoinIndex<'_>> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidates)| {
            if index == shape.driver {
                return JoinIndex::default();
            }
            let mut by_values = JoinIndex::default();
            for &candidate in candidates {
                by_values
                    .entry(join_values(&shape.joins[index], &candidate.entities))
                    .or_default()
                    .push(candidate);
            }
            by_values
        })
        .collect();
    let jobs = driver_groups(&candidates[shape.driver], shape)
        .into_iter()
        .flat_map(|(group, driven)| {
            contexts.iter().map(move |values| {
                let mut context = group.clone();
                context.extend(values);
                (context, driven.clone())
            })
        });
    jobs.map(|(context, driven)| expand_job(step, &indexes, incomplete, &context, &driven))
        .collect()
}

/// One input's candidates, by their values for the dimensions it joins on,
/// so each job finds its match without scanning them all.
type JoinIndex<'a> = FxHashMap<Vec<Option<&'a str>>, Vec<&'a ArtifactInstance>>;

/// `entities`' values for `joins`, in order; `None` where one is unbound.
fn join_values<'a>(joins: &[String], entities: &'a EntityBinding) -> Vec<Option<&'a str>> {
    joins
        .iter()
        .map(|dimension| entities.get(dimension))
        .collect()
}

/// One job of `step`: the driver's `driven` artifacts, every other input
/// matched to `context`, and the gaps that leave it incomplete.
fn expand_job(
    step: &CompiledStep<'_>,
    indexes: &[JoinIndex<'_>],
    incomplete: &ArtifactSet,
    context: &EntityBinding,
    driven: &[ArtifactInstance],
) -> Expansion {
    let (invocation, operation, shape) = (step.invocation, step.operation, &step.shape);
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
            driven.to_vec()
        } else {
            let matches = indexes[index]
                .get(&join_values(&shape.joins[index], context))
                .map_or(&[][..], Vec::as_slice);
            match match_input(invocation, operation, index, matches, context) {
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
                .filter(|artifact| incomplete.contains(artifact))
                .map(|artifact| Gap::Blocked {
                    port: port.name.clone(),
                    artifact: artifact.clone(),
                }),
        );
        inputs.push(bound);
    }
    // Every artifact in a family has the same type, so the type inferred
    // statically for each output is the type of each job's artifact.
    let outputs = step
        .outputs
        .iter()
        .map(|(product, artifact_type)| ArtifactInstance {
            product: product.name.clone(),
            artifact_type: artifact_type.clone(),
            entities: context.clone(),
        })
        .collect();
    Expansion {
        inputs,
        outputs,
        gaps,
    }
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
    let mut group_index: FxHashMap<EntityBinding, usize> = FxHashMap::default();
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

/// The one candidate for input `index` among `matches`, those that agree
/// with a job's context on every dimension it joins on, or the gap left
/// when none or several do.
fn match_input(
    invocation: &Invocation,
    operation: &OperationDef,
    index: usize,
    matches: &[&ArtifactInstance],
    context: &EntityBinding,
) -> Result<ArtifactInstance, Gap> {
    let [artifact] = matches else {
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
        let mut seen = FxHashSet::default();
        for candidate in candidates {
            let value = candidate
                .entities
                .project(&binding.each)
                .expect("an artifact binds every dimension of its product");
            if seen.insert(value.clone()) {
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
    artifact_producers: &ArtifactMap<usize>,
) -> Job {
    let dependencies: BTreeSet<_> = inputs
        .iter()
        .flatten()
        .filter_map(|input| artifact_producers.get(input).copied())
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
