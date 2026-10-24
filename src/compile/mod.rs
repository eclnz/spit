//! Compile a pipeline: check everything its text determines, with no
//! inventory: declarations, stages, each step's operation, inputs, shape and
//! inferred types, and cycles. Input rules and resolving jobs build on what
//! this returns.

mod declared;
mod definitions;
mod steps;
mod strict;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{Invocation, OperationDef, Pipeline, ProductDef, TypeChecking};
use crate::types::{Substitutions, TypeExpr};

use self::declared::check_declared;
use self::definitions::{
    check_stages, index_operations, index_producers, index_products, invocation_order,
};
use self::steps::step_shape;
pub(crate) use self::steps::StepShape;
use self::strict::check_strict;
use self::types::{check_calls, infer_types};

/// A pipeline whose declarations and steps hold without any inventory. Only
/// [`compile`] makes one, and only for a pipeline with no error, so a pipeline
/// that failed can never be resolved.
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
    /// The steps that checked, producers before consumers. With errors these
    /// are only part of the pipeline, which the editor may describe but
    /// nothing may resolve.
    pub(crate) steps: Vec<CompiledStep<'a>>,
    pub errors: Vec<(DefinitionSubject, ResolveError)>,
    /// Products and operations that are invalid or produced by a step that
    /// could not be checked. Anything using them is skipped rather than
    /// reported again.
    pub poisoned: BTreeSet<String>,
}

/// Check everything that depends only on the pipeline text: declarations,
/// each step's operation, inputs, dimensions and inferred types, cycles,
/// and under `types strict` that no type is left unknown.
/// Returns the first error [`collect_pipeline`] finds; the diagnostics give
/// them all.
pub fn validate_pipeline(pipeline: &Pipeline) -> Result<(), ResolveError> {
    compile(pipeline).map(|_| ())
}

/// Compile `pipeline`, or return the first error [`collect_pipeline`] finds.
pub(crate) fn compile(pipeline: &Pipeline) -> Result<CompiledPipeline<'_>, ResolveError> {
    let checked = collect_pipeline(pipeline);
    match checked.errors.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(CompiledPipeline {
            steps: checked.steps,
        }),
    }
}

/// Check the whole pipeline without an inventory, collecting every error.
/// This is the one check of a pipeline's text: [`compile`] and
/// [`validate_pipeline`] read their answer from it.
pub(crate) fn collect_pipeline(pipeline: &Pipeline) -> PipelineCheck<'_> {
    let mut errors = Vec::new();
    let mut poisoned = BTreeSet::new();
    let mut duplicated = BTreeSet::new();
    let products = index_products(
        &pipeline.products,
        &pipeline.invocations,
        &mut errors,
        &mut poisoned,
        &mut duplicated,
    );
    let operations = index_operations(&pipeline.operations, &mut errors, &mut poisoned);
    let producers = index_producers(
        &pipeline.invocations,
        &products,
        &operations,
        &duplicated,
        &mut errors,
        &mut poisoned,
    );
    check_stages(pipeline, &mut errors);
    check_declared(pipeline, &mut errors, &mut poisoned);
    // Source order is no dependency order, so a step in a cycle is not
    // checked at all, and the steps that read from it are skipped below.
    let order = invocation_order(&pipeline.invocations, &producers);
    for cycle in order.cycles {
        for &step in &cycle.steps {
            poisoned.extend(pipeline.invocations[step].outputs.iter().cloned());
        }
        errors.push(cycle.into_error());
    }
    // Inferred intermediate types are also part of the reusable pipeline
    // contract. Check them in dependency order so a type error does not depend
    // on whether an inventory happens to contain concrete source artifacts.
    // A step that fails, or uses something that failed, quiets its consumers.
    let mut inferred_types = BTreeMap::new();
    let mut steps = Vec::new();
    for &index in &order.steps {
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
    check_calls(
        &pipeline.calls,
        &operations,
        &products,
        &inferred_types,
        &mut errors,
        &mut poisoned,
    );
    if pipeline.types == TypeChecking::Strict {
        check_strict(pipeline, &operations, &steps, &mut errors, &poisoned);
    }
    PipelineCheck {
        steps,
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

/// Check one step, or give its first fault. Its shape is checked only once
/// its types hold: a product of the wrong type at a port is most often an
/// argument in the wrong place, and a shape worked out from that binding
/// would only report the same mistake again.
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
