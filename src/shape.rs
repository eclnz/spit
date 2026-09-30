//! How a step's input bindings shape its jobs: which input drives the step,
//! and which dimensions its outputs take. Shared by lowering, which infers
//! the dimensions of a flow step's outputs, and by the resolver.

use std::cmp::Reverse;
use std::collections::BTreeSet;

use crate::model::{Cardinality, InputBinding, InputPort, OperationDef};

/// Supply a many input's omitted `@ vary` from the operation contract.
/// Explicit selectors remain unchanged so the contract check can reject a
/// mismatch.
pub(crate) fn effective_binding(
    binding: &InputBinding,
    port: &InputPort,
    operation: &OperationDef,
) -> InputBinding {
    let mut effective = binding.clone();
    if port.cardinality == Cardinality::Many && effective.vary.is_empty() {
        effective.vary.clone_from(&operation.aggregated_dimensions);
    }
    effective
}

/// A binding's product dimensions, less any `where` pins, and whether it
/// takes many artifacts.
pub(crate) struct BoundInput<'a> {
    pub(crate) binding: &'a InputBinding,
    pub(crate) dimensions: Vec<String>,
    pub(crate) many: bool,
}

impl BoundInput<'_> {
    /// The dimensions this input is matched on when it does not drive,
    /// including any it broadcasts.
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

    /// Whether this input may drive a step: one matched on fewer dimensions
    /// or broadcast over some cannot.
    pub(crate) fn can_drive(&self) -> bool {
        self.binding.same.is_none() && self.binding.each.is_empty()
    }
}

/// The dimensions the inputs broadcast with `@ each(...)`, in port order.
pub(crate) fn broadcast_dimensions(inputs: &[BoundInput<'_>]) -> Vec<String> {
    let mut dimensions: Vec<String> = Vec::new();
    for dimension in inputs.iter().flat_map(|input| &input.binding.each) {
        if !dimensions.contains(dimension) {
            dimensions.push(dimension.clone());
        }
    }
    dimensions
}

/// The driving input and the dimensions it groups by, or `None` when no
/// single-artifact input has every dimension the others match on besides
/// those broadcast. The driver is the input with the most dimensions, so the
/// order of an operation's ports never changes which jobs exist.
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

/// The dimensions of a step's outputs: `groups`, the driver's from
/// [`step_driver`], then any dimensions broadcast by another input.
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
