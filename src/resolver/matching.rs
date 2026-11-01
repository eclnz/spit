//! Expanding a checked step into jobs: one per driving artifact or group,
//! with every other input matched to that job.

use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compile::{CompiledStep, StepShape};
use crate::error::{NearMiss, PortSite, ResolveError};
use crate::model::{
    near_reason, ArtifactId, Artifacts, Cardinality, EntityBinding, Gap, Invocation, Job, JobId,
    OperationDef, StepId,
};

/// One job of a step: its inputs, and the context each of its outputs
/// binds. `inputs` is complete only when `gaps` is empty.
pub(super) struct Expansion {
    pub(super) inputs: Vec<Vec<ArtifactId>>,
    pub(super) context: EntityBinding,
    pub(super) gaps: Vec<Gap>,
}

/// Enumerate one step's jobs: one per driving artifact, or per group of the
/// many input, with every other input matched to that job's context.
pub(super) fn expand_step(
    step: &CompiledStep<'_>,
    artifacts: &Artifacts,
    families: &[Vec<ArtifactId>],
    incomplete: &[bool],
    partial: bool,
) -> Vec<Expansion> {
    let (invocation, shape) = (step.invocation, &step.shape);
    let candidates: Vec<Vec<ArtifactId>> = invocation
        .inputs
        .iter()
        .map(|binding| {
            artifacts
                .product_number(binding.product_name())
                .map_or(&[][..], |number| &families[number as usize])
                .iter()
                .copied()
                .filter(|&artifact| {
                    let entities = artifacts.entities(artifact);
                    binding
                        .pinned
                        .iter()
                        .all(|(dimension, value)| entities.get(dimension) == Some(value.as_str()))
                })
                .collect()
        })
        .collect();
    let contexts = broadcast_contexts(invocation, artifacts, &candidates);
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
                    .entry(join_values(
                        &shape.joins[index],
                        artifacts.entities(candidate),
                    ))
                    .or_default()
                    .push(candidate);
            }
            by_values
        })
        .collect();
    let jobs = driver_groups(artifacts, &candidates[shape.driver], shape)
        .into_iter()
        .flat_map(|(group, driven)| {
            contexts.iter().map(move |values| {
                let mut context = group.clone();
                context.extend(values);
                (context, driven.clone())
            })
        });
    let availability = Availability {
        incomplete,
        partial,
    };
    jobs.map(|(context, driven)| {
        expand_job(
            step,
            artifacts,
            &candidates,
            &indexes,
            availability,
            context,
            driven,
        )
    })
    .collect()
}

/// One input's candidates, by their values for the dimensions it joins on,
/// so each job finds its match without scanning them all.
type JoinIndex<'a> = FxHashMap<Vec<Option<&'a str>>, Vec<ArtifactId>>;

#[derive(Clone, Copy)]
struct Availability<'a> {
    incomplete: &'a [bool],
    partial: bool,
}

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
    artifacts: &Artifacts,
    candidates: &[Vec<ArtifactId>],
    indexes: &[JoinIndex<'_>],
    availability: Availability<'_>,
    context: EntityBinding,
    mut driven: Vec<ArtifactId>,
) -> Expansion {
    let (invocation, operation, shape) = (step.invocation, step.operation, &step.shape);
    let Availability {
        incomplete,
        partial,
    } = availability;
    let mut gaps = Vec::new();
    if partial
        && operation.inputs[shape.driver].cardinality == Cardinality::Many
        && driven.iter().any(|artifact| !incomplete[artifact.index()])
    {
        driven.retain(|artifact| !incomplete[artifact.index()]);
    }
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
    // What tells the artifacts of the driver's group apart, which the group
    // of every other many input must have as well.
    let paired = if operation.inputs.len() > 1
        && operation
            .inputs
            .iter()
            .enumerate()
            .any(|(index, port)| index != shape.driver && port.cardinality == Cardinality::Many)
    {
        driven.clone()
    } else {
        Vec::new()
    };
    let mut inputs = Vec::new();
    for (index, port) in operation.inputs.iter().enumerate() {
        let bound = if index == shape.driver {
            std::mem::take(&mut driven)
        } else if port.cardinality == Cardinality::Many {
            let group = indexes[index]
                .get(&join_values(&shape.joins[index], &context))
                .map_or(&[][..], Vec::as_slice);
            gaps.extend(unpaired(step, artifacts, index, &paired, group, &context));
            group.to_vec()
        } else {
            let matches = indexes[index]
                .get(&join_values(&shape.joins[index], &context))
                .map_or(&[][..], Vec::as_slice);
            match match_input(
                port_site(invocation, operation, index),
                matches,
                &candidates[index],
                &shape.joins[index],
                artifacts,
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
                .filter(|artifact| incomplete[artifact.index()])
                .map(|&artifact| Gap::Blocked {
                    port: port.name.clone(),
                    artifact: artifacts.get(artifact).to_instance(),
                }),
        );
        inputs.push(bound);
    }
    Expansion {
        inputs,
        context,
        gaps,
    }
}

/// The gaps left when the many input `index` holds `group` where the driver
/// holds `driven`: an artifact of one with no partner of the same
/// `varied` values in the other.
fn unpaired(
    step: &CompiledStep<'_>,
    artifacts: &Artifacts,
    index: usize,
    driven: &[ArtifactId],
    group: &[ArtifactId],
    context: &EntityBinding,
) -> Vec<Gap> {
    let (invocation, operation, shape) = (step.invocation, step.operation, &step.shape);
    let key = |artifact: ArtifactId| join_values(&shape.varied, artifacts.entities(artifact));
    let held: FxHashSet<_> = group.iter().map(|&artifact| key(artifact)).collect();
    let wanted: FxHashSet<_> = driven.iter().map(|&artifact| key(artifact)).collect();
    let missing = |site: PortSite, from: &[ArtifactId], other: &FxHashSet<Vec<Option<&str>>>| {
        from.iter()
            .filter(|&&artifact| !other.contains(&key(artifact)))
            .map(|&artifact| {
                let mut missing = context.clone();
                missing.extend(
                    &artifacts
                        .entities(artifact)
                        .project(&shape.varied)
                        .expect("an artifact binds every dimension of its product"),
                );
                Gap::Unmatched(ResolveError::MissingInput {
                    site: site.clone(),
                    context: Box::new(missing),
                    near: None,
                })
            })
            .collect::<Vec<_>>()
    };
    let mut gaps = missing(port_site(invocation, operation, index), driven, &held);
    gaps.extend(missing(
        port_site(invocation, operation, shape.driver),
        group,
        &wanted,
    ));
    gaps
}

/// The driver's artifacts grouped by the step's groups: one artifact per
/// group for a preserve step, or a collection per group for an aggregate
/// one. Families are sorted, so groups keep the order of their first
/// artifact and each collection is in natural entity order.
fn driver_groups(
    artifacts: &Artifacts,
    candidates: &[ArtifactId],
    shape: &StepShape,
) -> Vec<(EntityBinding, Vec<ArtifactId>)> {
    let mut groups: Vec<(EntityBinding, Vec<ArtifactId>)> = Vec::new();
    let mut group_index: FxHashMap<EntityBinding, usize> = FxHashMap::default();
    for &artifact in candidates {
        let context = artifacts
            .entities(artifact)
            .project(&shape.groups)
            .expect("an artifact binds every dimension of its product");
        let index = *group_index.entry(context.clone()).or_insert_with(|| {
            groups.push((context, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push(artifact);
    }
    groups
}

/// The one candidate for input `index` among `matches`, those that agree
/// with a job's context on every dimension it joins on, or the gap left
/// when none or several do.
fn match_input(
    site: PortSite,
    matches: &[ArtifactId],
    candidates: &[ArtifactId],
    joins: &[String],
    artifacts: &Artifacts,
    context: &EntityBinding,
) -> Result<ArtifactId, Gap> {
    let [artifact] = matches else {
        let context = Box::new(context.clone());
        return Err(Gap::Unmatched(if matches.is_empty() {
            let near = candidates.iter().find_map(|candidate| {
                let binding = artifacts.entities(*candidate);
                let mut difference = None;
                for dimension in joins {
                    let (Some(found), Some(wanted)) =
                        (binding.get(dimension), context.get(dimension))
                    else {
                        return None;
                    };
                    if found == wanted {
                        continue;
                    }
                    let reason = near_reason(found, wanted)?;
                    if difference.is_some() {
                        return None;
                    }
                    difference = Some((dimension.clone(), wanted.to_owned(), reason));
                }
                let (dimension, wanted, reason) = difference?;
                Some(Box::new(NearMiss {
                    artifact: artifacts.get(*candidate).to_instance(),
                    dimension,
                    wanted,
                    reason,
                }))
            });
            ResolveError::MissingInput {
                site,
                context,
                near,
            }
        } else {
            ResolveError::AmbiguousInput { site, context }
        }));
    };
    Ok(*artifact)
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

/// Every combination of the values the inputs broadcast with `@each(...)`:
/// for each such input, the values present in its product, in natural order.
/// A step without broadcasts has one empty combination.
fn broadcast_contexts(
    invocation: &Invocation,
    artifacts: &Artifacts,
    candidates: &[Vec<ArtifactId>],
) -> Vec<EntityBinding> {
    let mut contexts = vec![EntityBinding::default()];
    for (binding, candidates) in invocation.inputs.iter().zip(candidates) {
        if binding.each.is_empty() {
            continue;
        }
        let mut values: Vec<EntityBinding> = Vec::new();
        let mut seen = FxHashSet::default();
        for &candidate in candidates {
            let value = artifacts
                .entities(candidate)
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
    id: JobId,
    step: StepId,
    inputs: Vec<Vec<ArtifactId>>,
    outputs: Vec<ArtifactId>,
    producers: &[Option<JobId>],
) -> Job {
    let dependencies: BTreeSet<_> = inputs
        .iter()
        .flatten()
        .filter_map(|input| producers[input.index()])
        .collect();
    Job {
        id,
        step,
        inputs,
        outputs,
        dependencies: dependencies.into_iter().collect(),
    }
}
