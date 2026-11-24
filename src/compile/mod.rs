mod declared;
mod definitions;
mod steps;
mod strict;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use crate::check::collect_checks;
use crate::command::collect_commands;
use crate::model::{DefinitionSubject, ResolveError};
use crate::model::{Invocation, OperationDef, Pipeline, PipelineIndex, ProductDef, TypeChecking};
use crate::parser::SourceMap;
use crate::resources::collect_resources;
use crate::review::collect_reviews;
use crate::template::CommandError;
use crate::types::{Substitutions, TypeExpr};

use self::declared::check_declared;
use self::definitions::{
    check_stages, index_operations, index_producers, index_products, invocation_order,
};
use self::steps::step_shape;
pub(crate) use self::steps::StepShape;
use self::strict::check_strict;
use self::types::{check_calls, infer_types};

pub(crate) struct CompiledPipeline<'a> {
    pub(crate) steps: Vec<CompiledStep<'a>>,
}

pub(crate) struct CompiledStep<'a> {
    pub(crate) invocation: &'a Invocation,
    pub(crate) operation: &'a OperationDef,
    pub(crate) outputs: Vec<(&'a ProductDef, TypeExpr)>,
    pub(crate) substitutions: Substitutions,
    pub(crate) shape: StepShape,
}

pub(crate) struct DefinitionCheck<'a> {
    pub(crate) steps: Vec<CompiledStep<'a>>,
    pub errors: Vec<(DefinitionSubject, ResolveError)>,
    pub poisoned: BTreeSet<String>,
}

pub fn validate_pipeline(pipeline: &Pipeline) -> Result<(), ResolveError> {
    compile(pipeline).map(|_| ())
}

pub(crate) fn compile(pipeline: &Pipeline) -> Result<CompiledPipeline<'_>, ResolveError> {
    let checked = collect_definitions(pipeline);
    match checked.errors.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(CompiledPipeline {
            steps: checked.steps,
        }),
    }
}

pub(crate) fn collect_definitions(pipeline: &Pipeline) -> DefinitionCheck<'_> {
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
    let order = invocation_order(&pipeline.invocations, &producers);
    for cycle in order.cycles {
        for &step in &cycle.steps {
            poisoned.extend(pipeline.invocations[step].outputs.iter().cloned());
        }
        errors.push(cycle.into_error());
    }
    let folder_inputs = pipeline
        .operations
        .iter()
        .any(|operation| {
            operation
                .outputs
                .iter()
                .any(|output| output.seed.is_some() || output.view)
        })
        .then(|| PipelineIndex::new(pipeline));
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
        match validate_invocation(
            invocation,
            &products,
            &operations,
            &inferred_types,
            folder_inputs.as_ref(),
        ) {
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
    DefinitionCheck {
        steps,
        errors,
        poisoned,
    }
}

pub fn validate_commands(pipeline: &Pipeline) -> Result<(), CommandError> {
    let operations = operations_by_name(pipeline);
    let lines = SourceMap::default();
    let errors = collect_command_lines(pipeline, &operations, &lines, &BTreeSet::new());
    match errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub(crate) fn operations_by_name(pipeline: &Pipeline) -> BTreeMap<&str, &OperationDef> {
    pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect()
}

pub(crate) fn collect_command_lines(
    pipeline: &Pipeline,
    operations: &BTreeMap<&str, &OperationDef>,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<CommandError> {
    let mut errors = collect_commands(pipeline, operations, lines, skip);
    errors.extend(collect_resources(pipeline, operations, lines, skip));
    errors.extend(collect_checks(pipeline, lines, skip));
    errors.extend(collect_reviews(pipeline, lines, skip));
    errors
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
    folder_inputs: Option<&PipelineIndex<'_>>,
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
    if let Some(index) = folder_inputs {
        for output in &operation.outputs {
            let Some(seed) = &output.seed else { continue };
            for (port, binding) in operation.inputs.iter().zip(&invocation.inputs) {
                let product = binding.product_name();
                if port.name == *seed && (!index.is_folder(product) || index.is_view(product)) {
                    return Err(unsupported(
                        operation,
                        format!(
                        "`from {seed}` on output `{}` requires a folder source or an owned folder output; `{product}` is a file or directory view",
                        output.name
                    ),
                    ));
                }
            }
        }
    }
    let (inferred, substitutions) =
        infer_types(invocation, operation, products, &outputs, inferred_types)?;
    let shape = step_shape(invocation, operation, products, &outputs)?;
    if operation.outputs[0].view {
        let Some(index) = folder_inputs else {
            unreachable!("directory view declarations require a folder index");
        };
        let dimensions = &outputs[0].dimensions;
        let mut members = BTreeSet::new();
        for (binding, join) in invocation.inputs.iter().zip(&shape.joins) {
            let product = binding.product_name();
            if index.is_source(product) || index.is_folder(product) {
                return Err(unsupported(
                    operation,
                    format!("directory view member `{product}` must be a generated file"),
                ));
            }
            if !members.insert(product) {
                return Err(unsupported(
                    operation,
                    format!("directory view repeats member product `{product}`"),
                ));
            }
            let declared = &find_product(products, product)?.dimensions;
            if declared != dimensions || join != dimensions {
                return Err(unsupported(
                    operation,
                    format!(
                        "directory view member `{product}` must match the view’s full context [{}]",
                        dimensions.join(", ")
                    ),
                ));
            }
        }
    }
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
