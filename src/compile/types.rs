//! Type checking of steps: unifying each port's type with the product bound
//! to it, and inferring the types of the products a step makes.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, PortSite, ResolveError, TypeConflict};
use crate::model::{Call, CallId, Invocation, OperationDef, ProductDef};
use crate::types::{Substitutions, TypeExpr, TypeUnifyError};

use super::find_product;

/// Unify each port's type with the product bound to it, then infer the type
/// of each output: its port's type with every variable substituted, or the
/// product's declared type when the port leaves it unknown.
pub(super) fn infer_types(
    invocation: &Invocation,
    operation: &OperationDef,
    products: &BTreeMap<&str, &ProductDef>,
    outputs: &[&ProductDef],
    inferred_types: &BTreeMap<String, TypeExpr>,
) -> Result<(Vec<TypeExpr>, Substitutions), ResolveError> {
    let output_product = invocation.output_product();
    let mut substitutions = Substitutions::default();
    for (port, binding) in operation.inputs.iter().zip(&invocation.inputs) {
        let product = find_product(products, binding.product_name())?;
        unify_port(
            &mut substitutions,
            operation,
            output_product,
            &port.name,
            &product.name,
            &port.artifact_type,
            inferred_types
                .get(&product.name)
                .unwrap_or(&product.artifact_type),
        )?;
    }
    for (port, output) in operation.outputs.iter().zip(outputs) {
        unify_port(
            &mut substitutions,
            operation,
            output_product,
            &port.name,
            &output.name,
            &port.artifact_type,
            &output.artifact_type,
        )?;
    }
    let types = operation
        .outputs
        .iter()
        .zip(outputs)
        .map(|(port, output)| {
            let inferred = substitutions
                .substitute(&port.artifact_type)
                .erase_variables();
            if inferred == TypeExpr::Unknown {
                output.artifact_type.clone()
            } else {
                inferred
            }
        })
        .collect();
    Ok((types, substitutions))
}

/// Hold each call of an operation carried out by steps to the operation's
/// own signature: unify each port with the product the caller gave it or
/// the call made, under one set of variables per call. Its steps were
/// checked against their own operations, which says nothing of the types
/// the signature promises. A call that fails poisons its outputs; a call
/// that reads or makes something already poisoned is not checked again.
pub(super) fn check_calls(
    calls: &[Call],
    operations: &BTreeMap<&str, &OperationDef>,
    products: &BTreeMap<&str, &ProductDef>,
    inferred_types: &BTreeMap<String, TypeExpr>,
    errors: &mut Vec<(DefinitionSubject, ResolveError)>,
    poisoned: &mut BTreeSet<String>,
) {
    for (index, call) in calls.iter().enumerate() {
        let Some(operation) = operations.get(call.operation.as_str()) else {
            continue;
        };
        let failed = poisoned.contains(&call.operation)
            || call
                .inputs
                .iter()
                .chain(&call.outputs)
                .any(|product| poisoned.contains(product));
        if failed {
            continue;
        }
        if let Err(error) = check_call(call, operation, products, inferred_types) {
            errors.push((DefinitionSubject::Call(CallId::at(index)), error));
            poisoned.extend(call.outputs.iter().cloned());
        }
    }
}

fn check_call(
    call: &Call,
    operation: &OperationDef,
    products: &BTreeMap<&str, &ProductDef>,
    inferred_types: &BTreeMap<String, TypeExpr>,
) -> Result<(), ResolveError> {
    let output_product = call.outputs.first().map_or("", String::as_str);
    let mut substitutions = Substitutions::default();
    let inputs = operation
        .inputs
        .iter()
        .map(|port| (&port.name, &port.artifact_type));
    let outputs = operation
        .outputs
        .iter()
        .map(|port| (&port.name, &port.artifact_type));
    for ((port, expected), product) in inputs.zip(&call.inputs).chain(outputs.zip(&call.outputs)) {
        let actual = match inferred_types.get(product) {
            Some(inferred) => inferred,
            None => &find_product(products, product)?.artifact_type,
        };
        unify_port(
            &mut substitutions,
            operation,
            output_product,
            port,
            product,
            expected,
            actual,
        )?;
    }
    Ok(())
}

fn unify_port(
    substitutions: &mut Substitutions,
    operation: &OperationDef,
    output_product: &str,
    port: &str,
    product: &str,
    expected: &TypeExpr,
    actual: &TypeExpr,
) -> Result<(), ResolveError> {
    let site = || PortSite {
        operation: operation.name.clone(),
        output_product: output_product.to_owned(),
        port: port.to_owned(),
        product: product.to_owned(),
    };
    substitutions
        .unify(expected, actual)
        .map(|_| ())
        .map_err(|error| match error {
            TypeUnifyError::VariableConflict {
                variable,
                previous,
                required,
            } => ResolveError::TypeVariableConflict {
                site: site(),
                conflict: Box::new(TypeConflict {
                    variable,
                    previous,
                    required,
                }),
            },
            _ => ResolveError::TypeMismatch {
                site: site(),
                expected: Box::new(expected.clone()),
                found: Box::new(actual.clone()),
            },
        })
}
