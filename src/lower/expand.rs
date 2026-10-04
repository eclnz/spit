//! Calls to an operation with a body. Lowering replaces each call by the
//! body's steps, written over the caller's products, so every later stage
//! sees ordinary steps. A product the body makes for itself is filed under
//! the call's first output, as `instance::product`, so two calls never
//! share one; the body's outputs are the products the caller names.

use std::collections::BTreeSet;

use crate::model::{
    Call, CallId, InputBinding, Invocation, OperationDef, StepOrigin, StepOutput, DEFAULT_OUTPUT,
};
use crate::parser::{FlowStep, ParseError, Step};
use crate::types::TypeExpr;

use super::PipelineBuilder;

/// A step still to be added: the one written in the pipeline, or one a
/// call to an operation with a body made.
struct Pending {
    invocation: Invocation,
    outputs: Vec<StepOutput>,
}

impl PipelineBuilder {
    /// Add a flow step, written at `flow.step`. A call to an operation with
    /// a body adds the body's steps in its place, in order, and a call in
    /// a body is expanded in turn; a worklist, not recursion, so a deep
    /// nesting of bodies cannot overflow the stack.
    pub(super) fn add_flow_step(&mut self, flow: &FlowStep) -> Result<(), ParseError> {
        self.reject_intermediates(flow)?;
        let mut pending = vec![Pending {
            invocation: flow.invocation.clone(),
            outputs: flow.outputs.clone(),
        }];
        while let Some(next) = pending.pop() {
            let operation = self.called(&next.invocation, &flow.step)?;
            if self.pipeline.operations[operation].steps.is_empty() {
                self.add_step(
                    next.invocation,
                    &next.outputs,
                    &flow.step,
                    &flow.invocation.outputs,
                );
                continue;
            }
            let operation = self.pipeline.operations[operation].clone();
            let steps = self.expand(next, &operation, &flow.step)?;
            pending.extend(steps.into_iter().rev());
        }
        Ok(())
    }

    /// The position in `pipeline.operations` of the operation `invocation`
    /// calls, which must be declared before it.
    fn called(&self, invocation: &Invocation, step: &Step) -> Result<usize, ParseError> {
        let name = &invocation.operation;
        self.operation_at.get(name).copied().ok_or_else(|| {
            let place = step.operation();
            ParseError::new(
                place.line,
                format!("operation `{name}` must be declared before its first flow step"),
            )
            .within(&place)
            .with_kind(crate::parser::ParseErrorKind::UndeclaredOperation { name: name.clone() })
        })
    }

    /// Replace `call` of `operation` by the body's steps over the caller's
    /// products, recording the call. `step` is where the call written in
    /// the pipeline is, which the steps a nested call makes share.
    fn expand(
        &mut self,
        call: Pending,
        operation: &OperationDef,
        step: &Step,
    ) -> Result<Vec<Pending>, ParseError> {
        let Pending {
            invocation: caller,
            outputs: written,
        } = call;
        let name = &caller.operation;
        let at = || match &caller.origin {
            Some(origin) => origin.step.clone(),
            None => step.call(),
        };
        let mismatch = |what: &str, takes: usize, given: usize| {
            let place = at();
            ParseError::new(
                place.line,
                format!("operation `{name}` takes {takes} {what}, but this call gives {given}"),
            )
            .within(&place)
        };
        if caller.inputs.len() != operation.inputs.len() {
            return Err(mismatch(
                "inputs",
                operation.inputs.len(),
                caller.inputs.len(),
            ));
        }
        if caller.outputs.len() != operation.outputs.len() {
            return Err(mismatch(
                "outputs",
                operation.outputs.len(),
                caller.outputs.len(),
            ));
        }
        let instance = caller.output_product().to_owned();
        let id = CallId::at(self.pipeline.calls.len());
        self.pipeline.calls.push(Call {
            operation: name.clone(),
            instance: instance.clone(),
            parent: caller.origin.as_ref().map(|origin| origin.call),
            place: at(),
        });
        // The body's own products, under the call's name.
        let rename = |product: &str| match operation
            .outputs
            .iter()
            .position(|port| port.name == product)
        {
            Some(index) => caller.outputs[index].clone(),
            None => format!("{instance}::{product}"),
        };
        let mut steps = Vec::with_capacity(operation.steps.len());
        for body in &operation.steps {
            let mut inputs = Vec::with_capacity(body.invocation.inputs.len());
            for binding in &body.invocation.inputs {
                let port = operation
                    .inputs
                    .iter()
                    .position(|port| port.name == binding.product);
                inputs.push(match port {
                    Some(port) => merge(&caller.inputs[port], binding).map_err(|problem| {
                        let place = at();
                        ParseError::new(place.line, problem).within(&place)
                    })?,
                    None => InputBinding {
                        product: rename(&binding.product),
                        ..binding.clone()
                    },
                });
            }
            let outputs = body
                .outputs
                .iter()
                .map(|output| {
                    match operation
                        .outputs
                        .iter()
                        .position(|port| port.name == output.name)
                    {
                        // What the caller wrote of its product, else the
                        // type the operation declares for it.
                        Some(index) => {
                            let mut product = written[index].clone();
                            let declared = &operation.outputs[index].artifact_type;
                            if product.artifact_type.is_none()
                                && *declared != TypeExpr::Unknown
                                && !declared.has_variables()
                            {
                                product.artifact_type = Some(declared.clone());
                            }
                            product
                        }
                        None => StepOutput {
                            name: rename(&output.name),
                            ..output.clone()
                        },
                    }
                })
                .collect();
            for output in &body.outputs {
                if !operation
                    .outputs
                    .iter()
                    .any(|port| port.name == output.name)
                {
                    self.intermediates
                        .insert(rename(&output.name), (id, caller.operation.clone()));
                }
            }
            steps.push(Pending {
                invocation: Invocation {
                    operation: body.invocation.operation.clone(),
                    inputs,
                    outputs: body
                        .invocation
                        .outputs
                        .iter()
                        .map(|output| rename(output))
                        .collect(),
                    stage: caller.stage.clone(),
                    origin: Some(StepOrigin {
                        call: id,
                        step: body.place.clone(),
                    }),
                },
                outputs,
            });
        }
        Ok(steps)
    }

    /// Fail a step written in the pipeline that reads a product a call made
    /// for itself: only the body's outputs are the caller's to read.
    fn reject_intermediates(&self, flow: &FlowStep) -> Result<(), ParseError> {
        for (index, binding) in flow.invocation.inputs.iter().enumerate() {
            if let Some((call, operation)) = self.intermediates.get(&binding.product) {
                let call = &self.pipeline.calls[call.index()];
                let place = flow.step.input(index).unwrap_or_else(|| flow.step.call());
                return Err(ParseError::new(
                    place.line,
                    format!(
                        "`{}` is made inside the call `{} = {operation}(...)` on line {}; make it an output of `{operation}` to read it here",
                        binding.product, call.instance, call.place.line
                    ),
                )
                .within(&place));
            }
        }
        Ok(())
    }

    /// Check the body of `operation`, declared at `place`, before it is
    /// added: its outputs are named, its steps call operations declared
    /// before it, read only its ports and the products of earlier steps,
    /// and between them assign each output once.
    pub(super) fn check_body(
        &self,
        operation: &OperationDef,
        place: &crate::span::Place,
    ) -> Result<(), ParseError> {
        let name = &operation.name;
        let fail = |place: &crate::span::Place, message: String| {
            ParseError::new(place.line, message).within(place)
        };
        if let Some(port) = operation.outputs.iter().find(|port| {
            port.name == DEFAULT_OUTPUT
                || port.extension.is_some()
                || port.folder
                || port.beside.is_some()
        }) {
            let problem = if port.name == DEFAULT_OUTPUT {
                format!("operation `{name}` has a body, so it names each output its steps assign, as in `-> (result: Type)`")
            } else {
                format!("output `{}` of `{name}` takes its extension, folder and place from the step that writes it; write only its name and type", port.name)
            };
            return Err(fail(place, problem));
        }
        if operation.minimum_collection.is_some() {
            return Err(fail(
                place,
                format!("operation `{name}` has a body; write `@ min(...)` on the `many` input of the step that collects"),
            ));
        }
        let mut known: BTreeSet<&str> = operation
            .inputs
            .iter()
            .map(|port| port.name.as_str())
            .collect();
        for body in &operation.steps {
            let called = &body.invocation.operation;
            if !self.operation_at.contains_key(called) {
                return Err(fail(
                    &body.place,
                    format!("operation `{called}` must be declared before `{name}`, whose body calls it"),
                ));
            }
            for binding in &body.invocation.inputs {
                if !known.contains(binding.product.as_str()) {
                    return Err(fail(
                        &body.place,
                        format!(
                            "the body of `{name}` reads `{}`, which is neither one of its inputs nor made by an earlier step of it",
                            binding.product
                        ),
                    ));
                }
            }
            for output in &body.invocation.outputs {
                if !known.insert(output) {
                    return Err(fail(
                        &body.place,
                        format!(
                            "the body of `{name}` already has `{output}`; name each product once"
                        ),
                    ));
                }
            }
        }
        if let Some(port) = operation
            .outputs
            .iter()
            .find(|port| !known.contains(port.name.as_str()))
        {
            return Err(fail(
                place,
                format!(
                    "no step in the body of `{name}` makes its output `{}`",
                    port.name
                ),
            ));
        }
        Ok(())
    }
}

/// The binding a body step gives a port, over the caller's argument: the
/// caller's product, with the selectors of both.
fn merge(caller: &InputBinding, body: &InputBinding) -> Result<InputBinding, String> {
    let mut merged = caller.clone();
    merged.vary.extend(
        body.vary
            .iter()
            .filter(|dimension| !caller.vary.contains(dimension))
            .cloned(),
    );
    merged.each.extend(
        body.each
            .iter()
            .filter(|dimension| !caller.each.contains(dimension))
            .cloned(),
    );
    for (dimension, value) in &body.pinned {
        if let Some(previous) = merged.pinned.insert(dimension.clone(), value.clone()) {
            if previous != *value {
                return Err(format!(
                    "`{}` is given `@ where({dimension}={previous})` here, but the body reads it with `@ where({dimension}={value})`",
                    caller.product
                ));
            }
        }
    }
    match (&caller.same, &body.same) {
        (Some(_), Some(_)) if caller.same != body.same => {
            return Err(format!(
            "`{}` is given `@ same(...)` here, and the body reads it with another; give it once",
            caller.product
        ))
        }
        (None, Some(same)) => merged.same = Some(same.clone()),
        _ => {}
    }
    Ok(merged)
}
