//! Declaration and call explanations of operations.

use super::cardinality;
use super::signatures::product_signature;
use crate::compile::CompiledStep;
use crate::model::{Call, Cardinality, CommandRole, OperationDef, Pipeline, ProductDef};
use crate::render::written_step;
use crate::types::TypeExpr;
use std::collections::BTreeMap;

pub(super) fn operation_details(pipeline: &Pipeline, operation: &OperationDef) -> Vec<String> {
    let mut details = Vec::new();
    if !operation.steps.is_empty() {
        let steps: Vec<String> = operation
            .steps
            .iter()
            .map(|step| written_step(&step.invocation))
            .collect();
        details.push(format!(
            "Carried out by the steps in its body: {}",
            steps.join("\n")
        ));
        return details;
    }
    if operation
        .inputs
        .iter()
        .any(|port| port.cardinality == Cardinality::Many)
    {
        details.push("Groups a many input into one job per remaining context.".to_owned());
    } else {
        details.push(
            "Single-artifact inputs are matched for each job; outputs preserve the job's dimensions.".to_owned(),
        );
    }
    for command in pipeline
        .commands
        .iter()
        .filter(|command| command.operation == operation.name)
    {
        details.push(format!(
            "{}: {}",
            match command.role {
                CommandRole::Run => "Command",
                CommandRole::Verify => "Verify",
            },
            command.template
        ));
    }
    details
}

pub(super) fn call_details(
    step: &CompiledStep<'_>,
    products: &BTreeMap<&str, &ProductDef>,
    inferred: &BTreeMap<&str, &TypeExpr>,
) -> Vec<String> {
    let mut details = vec!["This call:".to_owned()];
    let mut selectors = Vec::new();
    for (port, binding) in step.operation.inputs.iter().zip(&step.invocation.inputs) {
        if let Some(product) = products.get(binding.product_name()) {
            let ty = inferred
                .get(product.name.as_str())
                .copied()
                .unwrap_or(&product.artifact_type);
            details.push(format!(
                "{} ← {} ({} input; expects {})",
                port.name,
                product_signature(product, ty),
                cardinality(port.cardinality),
                step.substitutions
                    .substitute(&port.artifact_type)
                    .erase_variables()
            ));
            if !binding.vary.is_empty() {
                selectors.push(format!(
                    "Collects {} across {}.",
                    port.name,
                    binding.vary.join(", ")
                ));
            }
            if !binding.pinned.is_empty() {
                selectors.push(format!(
                    "Pins {} to {}.",
                    port.name,
                    binding
                        .pinned
                        .iter()
                        .map(|(dimension, value)| format!("{dimension}={value}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Some(dimensions) = &binding.same {
                selectors.push(if dimensions.is_empty() {
                    format!("Matches {} without dimension keys.", port.name)
                } else {
                    format!("Matches {} on {}.", port.name, dimensions.join(", "))
                });
            }
            if !binding.each.is_empty() {
                selectors.push(format!(
                    "Broadcasts {} across {}.",
                    port.name,
                    binding.each.join(", ")
                ));
            }
        }
    }
    for (port, (product, ty)) in step.operation.outputs.iter().zip(&step.outputs) {
        details.push(format!(
            "{} → {}",
            port.name,
            product_signature(product, ty)
        ));
    }
    details.extend(selectors);
    if !step.substitutions.0.is_empty() {
        details.push(format!(
            "Type bindings: {}",
            step.substitutions
                .0
                .iter()
                .map(|(name, ty)| format!("{name} = {}", step.substitutions.substitute(ty)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    details
}

/// A composite's public ports and concrete products, rather than repeating
/// its generic body. A failed expanded step cannot supply inferred types.
pub(super) fn composite_details(
    operation: &OperationDef,
    call: &Call,
    products: &BTreeMap<&str, &ProductDef>,
    inferred: &BTreeMap<&str, &TypeExpr>,
) -> Vec<String> {
    let mut details = vec!["This call:".to_owned()];
    for (port, name) in operation.inputs.iter().zip(&call.inputs) {
        if let Some(product) = products.get(name.as_str()) {
            let ty = inferred
                .get(name.as_str())
                .copied()
                .unwrap_or(&product.artifact_type);
            details.push(format!(
                "{} ← {}",
                port.name,
                product_signature(product, ty)
            ));
        }
    }
    for (port, name) in operation.outputs.iter().zip(&call.outputs) {
        if let Some(product) = products.get(name.as_str()) {
            let ty = inferred
                .get(name.as_str())
                .copied()
                .unwrap_or(&product.artifact_type);
            details.push(format!(
                "{} → {}",
                port.name,
                product_signature(product, ty)
            ));
        }
    }
    if call
        .outputs
        .iter()
        .any(|name| !inferred.contains_key(name.as_str()))
    {
        details.push(
            "This call could not be fully checked; some inferred types are unavailable.".to_owned(),
        );
    }
    details
}
