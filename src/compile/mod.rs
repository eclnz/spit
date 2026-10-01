//! Compile a pipeline: check everything its text determines, with no
//! inventory: declarations, stages, each step's operation, inputs, shape and
//! inferred types, and cycles. Input rules and resolving jobs build on what
//! this returns.

mod definitions;
mod steps;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{Invocation, OperationDef, Pipeline, ProductDef};
use crate::types::{Substitutions, TypeExpr};

use self::definitions::{
    check_stages, index_operations, index_producers, index_products, invocation_order,
};
use self::steps::step_shape;
pub(crate) use self::steps::StepShape;
use self::types::infer_types;

/// A pipeline whose declarations and steps hold without any inventory.
pub(crate) struct CompiledPipeline<'a> {
    /// Each checked step, with every producer before its consumers.
    pub(crate) steps: Vec<CompiledStep<'a>>,
}

/// A step that checked: everything the resolver needs to expand it.
pub(crate) struct CompiledStep<'a> {
    pub(crate) invocation: &'a Invocation,
    pub(crate) operation: &'a OperationDef,
    /// Each output's product, with the type inferred for its artifacts.
    pub(crate) outputs: Vec<(&'a ProductDef, TypeExpr)>,
    /// Type variables bound at this invocation, for editor explanations.
    pub(crate) substitutions: Substitutions,
    pub(crate) shape: StepShape,
}

/// Every pipeline error, plus the names that failed or depend on a failure.
pub(crate) struct PipelineCheck<'a> {
    pub(crate) pipeline: CompiledPipeline<'a>,
    pub errors: Vec<(DefinitionSubject, ResolveError)>,
    /// Products and operations that are invalid or produced by a step that
    /// could not be checked. Anything using them is skipped rather than
    /// reported again.
    pub poisoned: BTreeSet<String>,
}

/// Check everything that depends only on the pipeline text: declarations,
/// each step's operation, inputs, dimensions and inferred types, and cycles.
/// Returns the first error.
pub fn validate_pipeline(pipeline: &Pipeline) -> Result<(), ResolveError> {
    compile(pipeline).map(|_| ())
}

/// Compile `pipeline`, or return its first error.
pub(crate) fn compile(pipeline: &Pipeline) -> Result<CompiledPipeline<'_>, ResolveError> {
    let checked = collect_pipeline(pipeline);
    match checked.errors.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(checked.pipeline),
    }
}

/// Check the whole pipeline without an inventory, collecting every error.
pub(crate) fn collect_pipeline(pipeline: &Pipeline) -> PipelineCheck<'_> {
    let mut errors = Vec::new();
    let mut poisoned = BTreeSet::new();
    let products = index_products(
        &pipeline.products,
        &pipeline.invocations,
        &mut errors,
        &mut poisoned,
    );
    let operations = index_operations(&pipeline.operations, &mut errors, &mut poisoned);
    let producers = index_producers(
        &pipeline.invocations,
        &products,
        &operations,
        &mut errors,
        &mut poisoned,
    );
    check_stages(pipeline, &producers, &mut errors);
    let order = match invocation_order(&pipeline.invocations, &producers) {
        Ok(order) => order,
        Err(cycle) => {
            errors.push(cycle.into_error());
            (0..pipeline.invocations.len()).collect()
        }
    };
    // Inferred intermediate types are also part of the reusable pipeline
    // contract. Check them in dependency order so a type error does not depend
    // on whether an inventory happens to contain concrete source artifacts.
    // A step that fails, or uses something that failed, quiets its consumers.
    let mut inferred_types = BTreeMap::new();
    let mut steps = Vec::new();
    for &index in &order {
        let invocation = &pipeline.invocations[index];
        let depends_on_failure = poisoned.contains(&invocation.operation)
            || invocation
                .outputs
                .iter()
                .any(|output| poisoned.contains(output))
            || invocation
                .inputs
                .iter()
                .any(|input| poisoned.contains(input.product_name()));
        if depends_on_failure {
            poisoned.extend(invocation.outputs.iter().cloned());
            continue;
        }
        match validate_invocation(invocation, &products, &operations, &inferred_types) {
            Ok(step) => {
                let names = invocation.outputs.iter().cloned();
                let types = step.outputs.iter().map(|(_, inferred)| inferred.clone());
                inferred_types.extend(names.zip(types));
                steps.push(step);
            }
            Err(error) => {
                errors.push((
                    DefinitionSubject::Invocation(invocation.output_product().to_owned()),
                    error,
                ));
                poisoned.extend(invocation.outputs.iter().cloned());
            }
        }
    }
    PipelineCheck {
        pipeline: CompiledPipeline { steps },
        errors,
        poisoned,
    }
}

pub(crate) fn find_product<'a>(
    products: &BTreeMap<&str, &'a ProductDef>,
    name: &str,
) -> Result<&'a ProductDef, ResolveError> {
    products
        .get(name)
        .copied()
        .ok_or_else(|| ResolveError::UnknownProduct {
            name: name.to_owned(),
        })
}

pub(crate) fn find_operation<'a>(
    operations: &BTreeMap<&str, &'a OperationDef>,
    name: &str,
) -> Result<&'a OperationDef, ResolveError> {
    operations
        .get(name)
        .copied()
        .ok_or_else(|| ResolveError::UnknownOperation {
            name: name.to_owned(),
        })
}

fn validate_invocation<'a>(
    invocation: &'a Invocation,
    products: &BTreeMap<&str, &'a ProductDef>,
    operations: &BTreeMap<&str, &'a OperationDef>,
    inferred_types: &BTreeMap<String, TypeExpr>,
) -> Result<CompiledStep<'a>, ResolveError> {
    let operation = find_operation(operations, &invocation.operation)?;
    let outputs = invocation
        .outputs
        .iter()
        .map(|name| find_product(products, name))
        .collect::<Result<Vec<_>, _>>()?;
    if invocation.inputs.len() != operation.inputs.len() {
        return Err(unsupported(
            operation,
            format!(
                "expected {} input bindings, found {}",
                operation.inputs.len(),
                invocation.inputs.len()
            ),
        ));
    }
    if invocation.outputs.len() != operation.outputs.len() {
        let names: Vec<_> = operation
            .outputs
            .iter()
            .map(|port| port.name.as_str())
            .collect();
        return Err(unsupported(
            operation,
            format!(
                "expected {} output products ({}), found {}",
                operation.outputs.len(),
                names.join(", "),
                invocation.outputs.len()
            ),
        ));
    }
    let (inferred, substitutions) =
        infer_types(invocation, operation, products, &outputs, inferred_types)?;
    let shape = step_shape(invocation, operation, products, &outputs)?;
    Ok(CompiledStep {
        invocation,
        operation,
        outputs: outputs.into_iter().zip(inferred).collect(),
        substitutions,
        shape,
    })
}

pub(crate) fn unsupported(operation: &OperationDef, detail: impl Into<String>) -> ResolveError {
    ResolveError::UnsupportedShapeRelationship {
        operation: operation.name.clone(),
        detail: detail.into(),
    }
}
