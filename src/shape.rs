use std::cmp::Reverse;
use std::collections::BTreeSet;

use crate::model::InputBinding;

pub(crate) struct BoundInput<'a> {
    pub(crate) binding: &'a InputBinding,
    pub(crate) dimensions: Vec<String>,
    pub(crate) many: bool,
}

impl BoundInput<'_> {
    pub(crate) fn joins(&self) -> Vec<String> {
        match &self.binding.same {
            Some(same) => {
                let mut joins = same.clone();
                joins.extend(
                    self.binding
                        .each
                        .iter()
                        .filter(|dimension| !same.contains(dimension))
                        .cloned(),
                );
                joins
            }
            None => self.dimensions.clone(),
        }
    }

    pub(crate) fn can_drive(&self) -> bool {
        self.binding.same.is_none() && self.binding.each.is_empty()
    }
}

pub(crate) fn broadcast_dimensions(inputs: &[BoundInput<'_>]) -> Vec<String> {
    let mut dimensions: Vec<String> = Vec::new();
    for dimension in inputs.iter().flat_map(|input| &input.binding.each) {
        if !dimensions.contains(dimension) {
            dimensions.push(dimension.clone());
        }
    }
    dimensions
}

pub(crate) fn step_driver(inputs: &[BoundInput<'_>]) -> Option<(usize, Vec<String>)> {
    if let Some(index) = inputs.iter().position(|input| input.many) {
        let vary = &inputs[index].binding.vary;
        let groups = inputs[index]
            .dimensions
            .iter()
            .filter(|dimension| !vary.contains(dimension))
            .cloned()
            .collect();
        return Some((index, groups));
    }
    let broadcast = broadcast_dimensions(inputs);
    let covers = |index: usize| {
        let mut dimensions = dimension_set(&inputs[index].dimensions);
        dimensions.extend(broadcast.iter().cloned());
        inputs
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .all(|(_, input)| dimension_set(&input.joins()).is_subset(&dimensions))
    };
    let index = (0..inputs.len())
        .filter(|index| inputs[*index].can_drive() && covers(*index))
        .min_by_key(|index| (Reverse(inputs[*index].dimensions.len()), *index))?;
    Some((index, inputs[index].dimensions.clone()))
}

pub(crate) fn step_context(inputs: &[BoundInput<'_>], groups: &[String]) -> Vec<String> {
    let mut context = groups.to_vec();
    for dimension in broadcast_dimensions(inputs) {
        if !context.contains(&dimension) {
            context.push(dimension);
        }
    }
    context
}

pub(crate) fn dimension_set(dimensions: &[String]) -> BTreeSet<String> {
    dimensions.iter().cloned().collect()
}
