//! Calls to an operation with a body. Lowering replaces each call by the
//! body's steps, written over the caller's products, so every later stage
//! sees ordinary steps. A product the body makes for itself is filed under
//! the call's first output, as `instance::product`, so two calls never
//! share one; the body's outputs are the products the caller names.

use std::collections::BTreeSet;

use rustc_hash::FxHashMap;

use crate::model::{
    Call, CallId, CheckUse, InputBinding, Invocation, OperationDef, Pipeline, Port, StepOrigin,
    StepOutput, DEFAULT_OUTPUT,
};
use crate::parser::{FlowStep, ParseError, Step};
use crate::types::TypeExpr;

use super::{Failure, PipelineBuilder};

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
    pub(super) fn add_flow_step(&mut self, flow: &FlowStep) -> Result<(), Failure> {
        self.reject_intermediates(flow).map_err(Failure::clean)?;
        let mut pending = vec![Pending {
            invocation: flow.invocation.clone(),
            outputs: flow.outputs.clone(),
        }];
        // Until a call to an operation with a body has changed the builder,
        // a step that fails leaves it as it was.
        let mut untouched = true;
        while let Some(next) = pending.pop() {
            let operation = self
                .called(&next.invocation, &flow.step)
                .map_err(|error| Failure {
                    error: Box::new(error),
                    clean: untouched,
                })?;
            if self.pipeline.operations[operation].steps.is_empty() {
                self.add_step(
                    next.invocation,
                    &next.outputs,
                    &flow.step,
                    &flow.invocation.outputs,
                );
                continue;
            }
            // The operations are read and the calls written: disjoint
            // fields, so the body is borrowed, not copied.
            let Pipeline {
                operations, calls, ..
            } = &mut self.pipeline;
            let steps = expand(
                next,
                &operations[operation],
                &flow.step,
                calls,
                &mut self.intermediates,
                &self.lines.imported,
            )
            .map_err(|failure| Failure {
                clean: untouched && failure.clean,
                ..failure
            })?;
            untouched = false;
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

    /// Fail a step written in the pipeline that reads a product a call made
    /// for itself: only the body's outputs are the caller's to read.
    fn reject_intermediates(&self, flow: &FlowStep) -> Result<(), ParseError> {
        for (index, binding) in flow.invocation.inputs.iter().enumerate() {
            if let Some(call) = self.intermediates.get(&binding.product) {
                let call = &self.pipeline.calls[call.index()];
                let operation = &call.operation;
                let place = flow.step.input(index).unwrap_or_else(|| flow.step.call());
                return Err(ParseError::new(
                    place.line,
                    format!(
                        "`{}` is made inside the call `{} = {operation}(...)` on line {}; make it an output of `{operation}` to read it here",
                        binding.product, call.outputs[0], call.place.line
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
    /// and between them assign each output once. An error in the header is
    /// the one error. An error in a step is that of the step, which the
    /// check then leaves out, as blanking its line would, and goes on to the
    /// steps after it.
    pub(super) fn check_body(
        &self,
        operation: &OperationDef,
        place: &crate::span::Place,
    ) -> Result<BodyCheck, ParseError> {
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
        let mut failed = Vec::new();
        for (position, body) in operation.steps.iter().enumerate() {
            let called = &body.invocation.operation;
            if !self.operation_at.contains_key(called) {
                failed.push((
                    position,
                    fail(
                        &body.place,
                        format!("operation `{called}` must be declared before `{name}`, whose body calls it"),
                    ),
                ));
                continue;
            }
            if let Some(binding) = body
                .invocation
                .inputs
                .iter()
                .find(|binding| !known.contains(binding.product.as_str()))
            {
                failed.push((
                    position,
                    fail(
                        &body.place,
                        format!(
                            "the body of `{name}` reads `{}`, which is neither one of its inputs nor made by an earlier step of it",
                            binding.product
                        ),
                    ),
                ));
                continue;
            }
            let outputs = &body.invocation.outputs;
            if let Some(repeated) = outputs.iter().position(|output| !known.insert(output)) {
                // The step is left out, and so are the outputs it made
                // before the one that repeats.
                for output in &outputs[..repeated] {
                    known.remove(output.as_str());
                }
                failed.push((
                    position,
                    fail(
                        &body.place,
                        format!(
                            "the body of `{name}` already has `{}`; name each product once",
                            outputs[repeated]
                        ),
                    ),
                ));
            }
        }
        let unmade = operation
            .outputs
            .iter()
            .find(|port| !known.contains(port.name.as_str()))
            .map(|port| {
                fail(
                    place,
                    format!(
                        "no step in the body of `{name}` makes its output `{}`",
                        port.name
                    ),
                )
            });
        Ok(BodyCheck { failed, unmade })
    }
}

/// What checking the steps of an operation's body found.
pub(super) struct BodyCheck {
    /// The steps that fail, by position, each with its error, in order.
    pub(super) failed: Vec<(usize, ParseError)>,
    /// An output of the operation that no step left in the body makes.
    pub(super) unmade: Option<ParseError>,
}

/// Replace `call` of `operation` by the body's steps over the caller's
/// products, recording the call. The call is recorded once its steps are
/// made, so an error leaves `calls` and `intermediates` as they were, which
/// the error says. `step` is where the call written in
/// the pipeline is, which the steps a nested call makes share.
fn expand(
    call: Pending,
    operation: &OperationDef,
    step: &Step,
    calls: &mut Vec<Call>,
    intermediates: &mut FxHashMap<String, CallId>,
    imported: &BTreeSet<String>,
) -> Result<Vec<Pending>, Failure> {
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
        return Err(Failure::clean(mismatch(
            "inputs",
            operation.inputs.len(),
            caller.inputs.len(),
        )));
    }
    if caller.outputs.len() != operation.outputs.len() {
        return Err(Failure::clean(mismatch(
            "outputs",
            operation.outputs.len(),
            caller.outputs.len(),
        )));
    }
    let instance = caller.output_product().to_owned();
    // The call files its own products under `instance::`, which an import
    // aliased `instance` would share.
    let prefix = format!("{instance}::");
    if let Some(taken) = imported
        .range(prefix.clone()..)
        .next()
        .filter(|name| name.starts_with(&prefix))
    {
        let place = at();
        return Err(Failure::clean(
            ParseError::new(
                place.line,
                format!(
                    "this call files the products it makes for itself under `{prefix}`, as an import does `{taken}`; rename the call's first output or the import's alias"
                ),
            )
            .within(&place),
        ));
    }
    let id = CallId::at(calls.len());
    // The body's own products, under the call's name.
    let rename = |product: &str| match operation
        .outputs
        .iter()
        .position(|port| port.name == product)
    {
        Some(index) => caller.outputs[index].clone(),
        None => format!("{instance}::{product}"),
    };
    // The checks on each of the operation's ports: its own, and those
    // the body this call is in gave the call, if any.
    let port_checks = |port: Port| -> Vec<CheckUse> {
        let declared = match port {
            Port::Input(index) => &operation.inputs[index].checks,
            Port::Output(index) => &operation.outputs[index].checks,
        };
        let given = caller
            .checks
            .iter()
            .filter(|(at, _)| *at == port)
            .map(|(_, check)| check);
        declared.iter().chain(given).cloned().collect()
    };
    let mut steps = Vec::with_capacity(operation.steps.len());
    // The products the body makes for itself, filed once the steps are made.
    let mut made = Vec::new();
    for body in &operation.steps {
        // A step that reads a port, or makes an output, runs its checks.
        let mut checks = Vec::new();
        let mut inputs = Vec::with_capacity(body.invocation.inputs.len());
        for (position, binding) in body.invocation.inputs.iter().enumerate() {
            let port = operation
                .inputs
                .iter()
                .position(|port| port.name == binding.product);
            if let Some(port) = port {
                checks.extend(
                    port_checks(Port::Input(port))
                        .into_iter()
                        .map(|check| (Port::Input(position), check)),
                );
            }
            inputs.push(match port {
                Some(port) => merge(&caller.inputs[port], binding).map_err(|problem| {
                    let place = at();
                    Failure::clean(ParseError::new(place.line, problem).within(&place))
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
        for (position, output) in body.invocation.outputs.iter().enumerate() {
            if let Some(port) = operation
                .outputs
                .iter()
                .position(|port| port.name == *output)
            {
                checks.extend(
                    port_checks(Port::Output(port))
                        .into_iter()
                        .map(|check| (Port::Output(position), check)),
                );
            }
        }
        for output in &body.outputs {
            if !operation
                .outputs
                .iter()
                .any(|port| port.name == output.name)
            {
                made.push(rename(&output.name));
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
                checks,
            },
            outputs,
        });
    }
    calls.push(Call {
        operation: name.clone(),
        outputs: caller.outputs.clone(),
        inputs: caller
            .inputs
            .iter()
            .map(|binding| binding.product.clone())
            .collect(),
        parent: caller.origin.as_ref().map(|origin| origin.call),
        place: at(),
    });
    intermediates.extend(made.into_iter().map(|product| (product, id)));
    Ok(steps)
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
