use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compile::{CompiledStep, StepShape};
use crate::model::{
    near_reason, ArtifactId, ArtifactSelection, Artifacts, Cardinality, EntityBinding, Gap,
    Invocation, Job, JobId, Judged, OperationDef, StepId,
};
use crate::model::{NearMiss, PortSite, ResolveError};

use super::reviews::{Block, Judging};

pub(super) struct Expansion {
    pub(super) inputs: Vec<Vec<ArtifactId>>,
    pub(super) context: EntityBinding,
    pub(super) gaps: Vec<Gap>,
    pub(super) left_out: bool,
    pub(super) held: Vec<Held>,
    pub(super) kept_from: Vec<(usize, ArtifactId)>,
}

pub(super) struct Held {
    pub(super) artifact: ArtifactId,
    pub(super) port: usize,
    pub(super) review: String,
    pub(super) judged: Judged,
}

pub(super) fn expand_step(
    step: &CompiledStep<'_>,
    artifacts: &Artifacts,
    families: &[Vec<ArtifactId>],
    incomplete: &[bool],
    partial: bool,
    judging: &Judging<'_>,
) -> Vec<Expansion> {
    let (invocation, shape) = (step.invocation, &step.shape);
    let mut candidates: Vec<Vec<ArtifactId>> = invocation
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
    for (port, dimensions) in &shape.selections {
        let last = matches!(
            invocation.inputs[*port].selection,
            Some(ArtifactSelection::Last(_))
        );
        select_artifacts(&mut candidates[*port], artifacts, dimensions, last);
    }
    let view = step.operation.outputs[0].view;
    let indexes: Vec<JoinIndex<'_>> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidates)| {
            if !view && index == shape.driver {
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
    let availability = Availability {
        incomplete,
        partial,
        judging,
    };
    if view {
        let seen: FxHashSet<_> = candidates
            .iter()
            .flatten()
            .map(|&artifact| artifacts.entities(artifact))
            .collect();
        let dimensions = &step.outputs[0].0.dimensions;
        let mut contexts: Vec<_> = seen.into_iter().collect();
        contexts.sort_unstable_by(|left, right| left.cmp_in(right, dimensions));
        return contexts
            .into_iter()
            .map(|context| {
                expand_job(
                    step,
                    artifacts,
                    &candidates,
                    &indexes,
                    availability,
                    context.clone(),
                    Vec::new(),
                )
            })
            .collect();
    }
    let contexts = broadcast_contexts(invocation, artifacts, &candidates);
    let jobs = driver_groups(artifacts, &candidates[shape.driver], shape)
        .into_iter()
        .flat_map(|(group, driven)| {
            contexts.iter().map(move |values| {
                let mut context = group.clone();
                context.extend(values);
                (context, driven.clone())
            })
        });
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

fn select_artifacts(
    candidates: &mut Vec<ArtifactId>,
    artifacts: &Artifacts,
    dimensions: &[String],
    last: bool,
) {
    if last {
        candidates.reverse();
    }
    let mut seen = FxHashSet::default();
    candidates.retain(|&artifact| {
        let key = artifacts
            .entities(artifact)
            .group_key(dimensions)
            .expect("an artifact binds every dimension of its product");
        seen.insert(key)
    });
    candidates.sort_unstable_by(|&left, &right| {
        artifacts
            .entities(left)
            .cmp_in(artifacts.entities(right), dimensions)
    });
}

type JoinIndex<'a> = FxHashMap<Vec<Option<&'a str>>, Vec<ArtifactId>>;

#[derive(Clone, Copy)]
struct Availability<'a> {
    incomplete: &'a [bool],
    partial: bool,
    judging: &'a Judging<'a>,
}

fn join_values<'a>(joins: &[String], entities: &'a EntityBinding) -> Vec<Option<&'a str>> {
    joins
        .iter()
        .map(|dimension| entities.get(dimension))
        .collect()
}

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
        judging,
    } = availability;
    let mut gaps = Vec::new();
    let mut reviewed = ReviewedInputs::default();
    if judging.active() {
        reviewed = review_inputs(step, artifacts, indexes, judging, &context, &driven);
        if reviewed.left_out {
            return Expansion {
                inputs: Vec::new(),
                context,
                gaps,
                left_out: true,
                held: reviewed.held,
                kept_from: reviewed.kept_from,
            };
        }
        driven.retain(|&artifact| reviewed.keeps(shape.driver, artifact, step, artifacts));
    }
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
        let bound = if !operation.outputs[0].view && index == shape.driver {
            std::mem::take(&mut driven)
        } else if port.cardinality == Cardinality::Many {
            let group = indexes[index]
                .get(&join_values(&shape.joins[index], &context))
                .map_or(&[][..], Vec::as_slice);
            let group: Vec<ArtifactId> = group
                .iter()
                .copied()
                .filter(|&artifact| reviewed.keeps(index, artifact, step, artifacts))
                .collect();
            gaps.extend(unpaired(step, artifacts, index, &paired, &group, &context));
            group
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
                    Vec::new()
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
        left_out: false,
        held: reviewed.held,
        kept_from: reviewed.kept_from,
    }
}

#[derive(Default)]
struct ReviewedInputs<'a> {
    dropped: FxHashSet<(usize, ArtifactId)>,
    keys: FxHashSet<Vec<Option<&'a str>>>,
    left_out: bool,
    held: Vec<Held>,
    kept_from: Vec<(usize, ArtifactId)>,
}

impl<'a> ReviewedInputs<'a> {
    fn keeps(
        &self,
        port: usize,
        artifact: ArtifactId,
        step: &CompiledStep<'_>,
        artifacts: &'a Artifacts,
    ) -> bool {
        !self.dropped.contains(&(port, artifact))
            && (self.keys.is_empty()
                || !self.keys.contains(&join_values(
                    &step.shape.varied,
                    artifacts.entities(artifact),
                )))
    }
}

fn review_inputs<'a>(
    step: &CompiledStep<'_>,
    artifacts: &'a Artifacts,
    indexes: &[JoinIndex<'_>],
    judging: &Judging<'_>,
    context: &EntityBinding,
    driven: &[ArtifactId],
) -> ReviewedInputs<'a> {
    let (operation, shape) = (step.operation, &step.shape);
    let mut reviewed = ReviewedInputs::default();
    let many: Vec<usize> = (0..operation.inputs.len())
        .filter(|&index| operation.inputs[index].cardinality == Cardinality::Many)
        .collect();
    let members = |index: usize| -> &[ArtifactId] {
        if !operation.outputs[0].view && index == shape.driver {
            return driven;
        }
        indexes[index]
            .get(&join_values(&shape.joins[index], context))
            .map_or(&[][..], Vec::as_slice)
    };
    for index in 0..operation.inputs.len() {
        let is_many = many.contains(&index);
        let members = members(index);
        if !is_many && members.len() != 1 {
            continue;
        }
        for &artifact in members {
            let Some(block) = judging.block(artifact, artifacts.entities(artifact), index) else {
                continue;
            };
            reviewed.kept_from.push((index, artifact));
            if let Block::Review { review, judged } = block {
                reviewed.held.push(Held {
                    artifact,
                    port: index,
                    review: review.to_owned(),
                    judged,
                });
            }
            if !is_many {
                reviewed.left_out = true;
            } else {
                reviewed.dropped.insert((index, artifact));
                if many.len() > 1 {
                    reviewed
                        .keys
                        .insert(join_values(&shape.varied, artifacts.entities(artifact)));
                }
            }
        }
    }
    if reviewed.dropped.is_empty() {
        return reviewed;
    }
    for &index in &many {
        let members = members(index);
        if !members.is_empty()
            && !members
                .iter()
                .any(|&artifact| reviewed.keeps(index, artifact, step, artifacts))
        {
            reviewed.left_out = true;
        }
    }
    reviewed
}

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

fn port_site(invocation: &Invocation, operation: &OperationDef, index: usize) -> PortSite {
    PortSite {
        operation: operation.name.clone(),
        output_product: invocation.output_product().to_owned(),
        port: operation.inputs[index].name.clone(),
        product: invocation.inputs[index].product.clone(),
    }
}

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
        missing: Vec::new(),
    }
}
