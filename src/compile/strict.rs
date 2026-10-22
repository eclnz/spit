//! `types strict`: once inference is done, every source, every port of an
//! operation the pipeline uses, and every product a step makes has a type
//! with no unknown part.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{OperationDef, Pipeline};
use crate::types::{Substitutions, TypeExpr};

use super::CompiledStep;

/// Report each type strict typing needs and the pipeline leaves unknown.
/// A step that reads an untyped product, or runs an operation with an
/// untyped port, is not reported again for the outputs that follow from it.
/// Names in `poisoned` already have an error and are skipped.
pub(super) fn check_strict(
    pipeline: &Pipeline,
    operations: &BTreeMap<&str, &OperationDef>,
    steps: &[CompiledStep<'_>],
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
    poisoned: &BTreeSet<String>,
) {
    let made: BTreeSet<&str> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| invocation.outputs.iter().map(String::as_str))
        .collect();
    // Names whose type is reported missing, so what follows from them is not.
    let mut untyped: BTreeSet<&str> = BTreeSet::new();
    for product in &pipeline.products {
        let name = product.name.as_str();
        if made.contains(name) || poisoned.contains(name) || is_known(&product.artifact_type) {
            continue;
        }
        untyped.insert(name);
        let detail = if product.artifact_type == TypeExpr::Unknown {
            let extension = product
                .extension
                .as_deref()
                .map_or_else(String::new, |extension| format!(" {extension}"));
            let folder = match (product.folder, product.extension.is_some()) {
                (false, _) => "",
                (true, true) => "/",
                (true, false) => " /",
            };
            let dimensions = if product.dimensions.is_empty() {
                String::new()
            } else {
                format!(" [{}]", product.dimensions.join(", "))
            };
            format!(
                "`types strict` needs a type on source `{name}`, as in `source {name} : Type{extension}{folder}{dimensions}`"
            )
        } else {
            format!(
                "`types strict` needs a type with no unknown part on source `{name}`, not {}",
                product.artifact_type
            )
        };
        errors.push((
            DefinitionSubject::Product(name.to_owned()),
            invalid(DefinitionSubject::Product(name.to_owned()), detail),
        ));
    }
    let used: BTreeSet<&str> = pipeline
        .invocations
        .iter()
        .map(|invocation| invocation.operation.as_str())
        .chain(pipeline.calls.iter().map(|call| call.operation.as_str()))
        .collect();
    for name in used {
        let Some(operation) = operations.get(name) else {
            continue;
        };
        if poisoned.contains(name) {
            continue;
        }
        let ports: Vec<String> = operation
            .inputs
            .iter()
            .map(|port| (&port.name, &port.artifact_type))
            .chain(
                operation
                    .outputs
                    .iter()
                    .map(|port| (&port.name, &port.artifact_type)),
            )
            // A variable is judged at each step, where inference fixes it
            // or leaves the product's type unknown.
            .filter(|(_, ty)| !is_known(ty))
            .map(|(port, _)| format!("`{port}`"))
            .collect();
        if ports.is_empty() {
            continue;
        }
        untyped.insert(name);
        let subject = DefinitionSubject::Operation(name.to_owned());
        let detail = format!(
            "`types strict` needs a type on every port of operation `{name}`, which {} {} without one: write `port: Type` for an input and `-> Type` for an output",
            if ports.len() == 1 { "has" } else { "has ports" },
            ports.join(", ")
        );
        errors.push((subject.clone(), invalid(subject, detail)));
    }
    for step in steps {
        let invocation = step.invocation;
        let follows = untyped.contains(invocation.operation.as_str())
            || invocation
                .inputs
                .iter()
                .any(|input| untyped.contains(input.product_name()));
        if follows {
            untyped.extend(invocation.outputs.iter().map(String::as_str));
            continue;
        }
        let ports = step.operation.outputs.iter().zip(&step.outputs);
        for (port, (product, inferred)) in ports {
            if is_known(inferred) {
                continue;
            }
            untyped.insert(product.name.as_str());
            let subject = DefinitionSubject::Invocation(product.name.clone());
            let detail = unknown_output(
                step,
                &port.artifact_type,
                &product.name,
                &product.dimensions,
            );
            errors.push((subject.clone(), invalid(subject, detail)));
        }
    }
}

/// Why the product `name` a step makes has no known type, and how to write
/// one: in place of each variable its inputs leave open, `Type`.
fn unknown_output(
    step: &CompiledStep<'_>,
    port_type: &TypeExpr,
    name: &str,
    dimensions: &[String],
) -> String {
    let operation = &step.operation.name;
    let given = step.substitutions.substitute(port_type);
    let open = given.variables();
    let why = if open.is_empty() {
        String::new()
    } else {
        let open = open.iter().map(|variable| format!("`${variable}`"));
        format!(
            ", and its inputs fix no {}",
            open.collect::<Vec<_>>().join(" or ")
        )
    };
    let written = Substitutions(
        open.into_iter()
            .map(|variable| (variable, TypeExpr::named("Type")))
            .collect(),
    )
    .substitute(&given);
    let dimensions = if dimensions.is_empty() {
        String::new()
    } else {
        format!(" [{}]", dimensions.join(", "))
    };
    // A step a call made is written in the body under its own name, which
    // the call's expansion prefixes.
    let written_name = match step.invocation.origin {
        Some(_) => name.rsplit("::").next().unwrap_or(name),
        None => name,
    };
    let declared = format!("{written_name} : {written}{dimensions}");
    let example = if step.invocation.outputs.len() == 1 {
        format!("`{declared} = {operation}(...)`")
    } else {
        format!("`{declared}` where the step names `{written_name}`")
    };
    format!(
        "`types strict` needs a known type for `{name}`, but `{operation}` gives it {given}{why}; write its type, as in {example}"
    )
}

fn invalid(subject: DefinitionSubject, detail: String) -> ResolveError {
    ResolveError::InvalidDefinition { subject, detail }
}

/// Whether `ty` has no unknown part. A variable counts as known.
fn is_known(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Unknown => false,
        TypeExpr::Applied { args, .. } => args.iter().all(is_known),
        TypeExpr::Named(_) | TypeExpr::Variable(_) => true,
    }
}
