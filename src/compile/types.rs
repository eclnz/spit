//! Type checking of steps: unifying each port's type with the product bound
//! to it, and inferring the types of the products a step makes.

use std::collections::BTreeMap;

use crate::error::{PortSite, ResolveError, TypeConflict};
use crate::model::{Invocation, OperationDef, ProductDef};
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
