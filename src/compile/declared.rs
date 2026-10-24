//! `type Name<Param, ...>`: a type written with a declared name takes as
//! many arguments as the declaration has parameters. Under `types strict`
//! a type written with arguments must be declared.

use std::collections::BTreeSet;

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{Pipeline, TypeChecking};
use crate::types::TypeExpr;

/// Report each declared type that is written with the wrong number of
/// arguments, and under `types strict` each type with arguments that is not
/// declared. A source, an operation or a step's product with a bad type is
/// reported once, and added to `poisoned`, so what uses it is not checked
/// against it.
pub(super) fn check_declared(
    pipeline: &Pipeline,
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) {
    let strict = pipeline.types == TypeChecking::Strict;
    if !strict && pipeline.type_decls.is_empty() {
        return;
    }
    let made: BTreeSet<&str> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| invocation.outputs.iter().map(String::as_str))
        .collect();
    let mut report = |subject: DefinitionSubject, name: &str, problem: String| {
        poisoned.insert(name.to_owned());
        errors.push((
            subject.clone(),
            ResolveError::InvalidDefinition {
                subject,
                detail: problem,
            },
        ));
    };
    for product in &pipeline.products {
        let Some(problem) = misuse(pipeline, &product.artifact_type, strict) else {
            continue;
        };
        let name = product.name.as_str();
        let (subject, what) = if made.contains(name) {
            (DefinitionSubject::Invocation(name.to_owned()), "product")
        } else {
            (DefinitionSubject::Product(name.to_owned()), "source")
        };
        report(subject, name, format!("{what} `{name}`: {problem}"));
    }
    for operation in &pipeline.operations {
        let inputs = operation
            .inputs
            .iter()
            .map(|port| (&port.name, &port.artifact_type));
        let outputs = operation
            .outputs
            .iter()
            .map(|port| (&port.name, &port.artifact_type));
        let bad = inputs
            .chain(outputs)
            .find_map(|(port, ty)| misuse(pipeline, ty, strict).map(|problem| (port, problem)));
        if let Some((port, problem)) = bad {
            let name = operation.name.as_str();
            let subject = DefinitionSubject::Operation(name.to_owned());
            report(
                subject,
                name,
                format!("operation `{name}`, port `{port}`: {problem}"),
            );
        }
    }
}

/// What is wrong with how `ty`, or a type inside it, is written.
fn misuse(pipeline: &Pipeline, ty: &TypeExpr, strict: bool) -> Option<String> {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        let (name, args): (&str, &[TypeExpr]) = match ty {
            TypeExpr::Named(name) => (name, &[]),
            TypeExpr::Applied { constructor, args } => (constructor, args),
            TypeExpr::Variable(_) | TypeExpr::Unknown => continue,
        };
        match pipeline.type_decls.get(name) {
            Some(params) if params.len() != args.len() => {
                return Some(format!(
                    "`{ty}` has {}, but `{name}` is declared `type {name}<{}>` and takes {}",
                    arguments(args.len()),
                    params.join(", "),
                    arguments(params.len()),
                ));
            }
            None if strict && !args.is_empty() => {
                return Some(format!(
                    "`types strict` needs `{name}` declared with a name for each of its arguments, as in `type {name}<{}>`",
                    vec!["Param"; args.len()].join(", "),
                ));
            }
            _ => {}
        }
        pending.extend(args);
    }
    None
}

/// `1 type argument`, `2 type arguments`.
fn arguments(count: usize) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} type argument{plural}")
}
