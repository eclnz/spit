use std::collections::BTreeSet;

use crate::model::{
    Call, CallId, CheckUse, CommandId, CommandRole, InputBinding, Invocation, OperationDef,
    Pipeline, Port, StepOrigin, StepOutput, DEFAULT_OUTPUT,
};
use crate::parser::error::LowerError;
use crate::parser::{FlowStep, ParseError, Step};
use crate::types::TypeExpr;

use super::{PipelineBuilder, Undo};

struct Pending {
    invocation: Invocation,
    outputs: Vec<StepOutput>,
}

impl PipelineBuilder {
    pub(super) fn add_flow_step(&mut self, flow: &FlowStep) -> Result<(), ParseError> {
        self.reject_intermediates(flow)?;
        let mark = (
            self.pipeline.products.len(),
            self.pipeline.invocations.len(),
            self.pipeline.calls.len(),
        );
        self.journal = Some(Vec::new());
        let added = self.expand_flow_step(flow);
        let journal = self
            .journal
            .take()
            .expect("a journal is kept while a step is added");
        if added.is_err() {
            self.roll_back(mark, journal);
        }
        added
    }

    fn roll_back(
        &mut self,
        (products, invocations, calls): (usize, usize, usize),
        journal: Vec<Undo>,
    ) {
        self.pipeline.products.truncate(products);
        self.pipeline.invocations.truncate(invocations);
        self.pipeline.calls.truncate(calls);
        for undo in journal.into_iter().rev() {
            match undo {
                Undo::Output(name, Some(before)) => {
                    self.outputs.insert(name, before);
                }
                Undo::Output(name, None) => {
                    self.outputs.remove(&name);
                }
                Undo::ProductPlace(name, Some(before)) => {
                    self.lines.products.insert(name, before);
                }
                Undo::ProductPlace(name, None) => {
                    self.lines.products.remove(&name);
                }
                Undo::ProductId(name) => {
                    self.product_ids.remove(&name);
                }
                Undo::Invocation(name, Some(before)) => {
                    self.lines.invocations.insert(name, before);
                }
                Undo::Invocation(name, None) => {
                    self.lines.invocations.remove(&name);
                }
                Undo::Intermediate(name, Some(before)) => {
                    self.intermediates.insert(name, before);
                }
                Undo::Intermediate(name, None) => {
                    self.intermediates.remove(&name);
                }
            }
        }
    }

    fn expand_flow_step(&mut self, flow: &FlowStep) -> Result<(), ParseError> {
        let mut pending = vec![Pending {
            invocation: Invocation {
                line: Some(flow.step.call().line),
                ..flow.invocation.clone()
            },
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
            let Pipeline {
                operations, calls, ..
            } = &mut self.pipeline;
            let Expansion { steps, made } = expand(
                next,
                &operations[operation],
                &flow.step,
                calls,
                &self.lines.imported,
            )?;
            for (product, call) in made {
                let before = self.intermediates.insert(product.clone(), call);
                self.record(|| Undo::Intermediate(product, before));
            }
            pending.extend(steps.into_iter().rev());
        }
        Ok(())
    }

    fn called(&self, invocation: &Invocation, step: &Step) -> Result<usize, ParseError> {
        let name = &invocation.operation;
        self.operation_at.get(name).copied().ok_or_else(|| {
            let place = step.operation();
            ParseError::of(
                place.line,
                crate::parser::ParseErrorKind::UndeclaredOperation { name: name.clone() },
            )
            .within(&place)
        })
    }

    fn reject_intermediates(&self, flow: &FlowStep) -> Result<(), ParseError> {
        for (index, binding) in flow.invocation.inputs.iter().enumerate() {
            if let Some(call) = self.intermediates.get(&binding.product) {
                let call = &self.pipeline.calls[call.index()];
                let operation = &call.operation;
                let place = flow.step.input(index).unwrap_or_else(|| flow.step.call());
                return Err(ParseError::of(
                    place.line,
                    LowerError::ProductMadeInsideCall {
                        product: binding.product.to_string(),
                        outputs: (call.outputs[0]).to_string(),
                        operation: operation.to_string(),
                        line: call.place.line,
                    },
                )
                .within(&place));
            }
        }
        Ok(())
    }

    pub(super) fn check_body(
        &self,
        operation: &OperationDef,
        place: &crate::span::Place,
    ) -> Result<BodyCheck, ParseError> {
        let name = &operation.name;
        let fail = |place: &crate::span::Place, error: LowerError| {
            ParseError::of(place.line, error).within(place)
        };
        if let Some(port) = operation.outputs.iter().find(|port| {
            port.name == DEFAULT_OUTPUT
                || port.extension.is_some()
                || port.folder
                || port.beside.is_some()
        }) {
            let problem = if port.name == DEFAULT_OUTPUT {
                LowerError::BodyOutputsUnnamed { name: name.clone() }
            } else {
                LowerError::BodyOutputDeclaresPlace {
                    port: port.name.clone(),
                    name: name.clone(),
                }
            };
            return Err(fail(place, problem));
        }
        if operation.minimum_collection.is_some() {
            return Err(fail(
                place,
                LowerError::BodyHasMinimum { name: name.clone() },
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
                        LowerError::BodyCallsUndeclared {
                            called: called.clone(),
                            name: name.clone(),
                        },
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
                        LowerError::BodyReadsUnknown {
                            name: name.clone(),
                            product: binding.product.clone(),
                        },
                    ),
                ));
                continue;
            }
            let outputs = &body.invocation.outputs;
            if let Some(repeated) = outputs.iter().position(|output| !known.insert(output)) {
                for output in &outputs[..repeated] {
                    known.remove(output.as_str());
                }
                failed.push((
                    position,
                    fail(
                        &body.place,
                        LowerError::BodyRepeatsProduct {
                            name: name.clone(),
                            output: outputs[repeated].clone(),
                        },
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
                    LowerError::BodyOutputUnmade {
                        name: name.clone(),
                        output: port.name.clone(),
                    },
                )
            });
        Ok(BodyCheck { failed, unmade })
    }
}

pub(super) struct BodyCheck {
    pub(super) failed: Vec<(usize, ParseError)>,
    pub(super) unmade: Option<ParseError>,
}

struct Expansion {
    steps: Vec<Pending>,
    made: Vec<(String, CallId)>,
}

fn expand(
    call: Pending,
    operation: &OperationDef,
    step: &Step,
    calls: &mut Vec<Call>,
    imported: &BTreeSet<String>,
) -> Result<Expansion, ParseError> {
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
        ParseError::of(
            place.line,
            LowerError::CallArgumentCount {
                name: name.to_string(),
                takes,
                what: what.to_string(),
                given,
            },
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
    let prefix = format!("{instance}::");
    if let Some(taken) = imported
        .range(prefix.clone()..)
        .next()
        .filter(|name| name.starts_with(&prefix))
    {
        let place = at();
        return Err(ParseError::of(
            place.line,
            LowerError::CallPrefixTaken {
                prefix: prefix.to_string(),
                taken: taken.to_string(),
            },
        )
        .within(&place));
    }
    let id = CallId::at(calls.len());
    let rename = |product: &str| match operation
        .outputs
        .iter()
        .position(|port| port.name == product)
    {
        Some(index) => caller.outputs[index].clone(),
        None => format!("{instance}::{product}"),
    };
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
    let port_reviews = |port: Port| -> Vec<String> {
        let declared = match port {
            Port::Input(index) => &operation.inputs[index].reviews,
            Port::Output(index) => &operation.outputs[index].reviews,
        };
        let given = caller
            .reviews
            .iter()
            .filter(|(at, _)| *at == port)
            .map(|(_, review)| review);
        declared.iter().chain(given).cloned().collect()
    };
    let mut steps = Vec::with_capacity(operation.steps.len());
    let mut made = Vec::new();
    for body in &operation.steps {
        let mut checks = body.invocation.checks.clone();
        let mut reviews = body.invocation.reviews.clone();
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
                reviews.extend(
                    port_reviews(Port::Input(port))
                        .into_iter()
                        .map(|review| (Port::Input(position), review)),
                );
            }
            inputs.push(match port {
                Some(port) => merge(&caller.inputs[port], binding).map_err(|problem| {
                    let place = at();
                    ParseError::of(place.line, problem).within(&place)
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
                reviews.extend(
                    port_reviews(Port::Output(port))
                        .into_iter()
                        .map(|review| (Port::Output(position), review)),
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
                line: Some(body.place.line),
                checks,
                reviews,
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
    Ok(Expansion {
        steps,
        made: made.into_iter().map(|product| (product, id)).collect(),
    })
}

fn merge(caller: &InputBinding, body: &InputBinding) -> Result<InputBinding, LowerError> {
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
                return Err(LowerError::WhereConflictsWithBody {
                    product: caller.product.clone(),
                    dimension: dimension.clone(),
                    previous,
                    value: value.clone(),
                });
            }
        }
    }
    match (&caller.same, &body.same) {
        (Some(_), Some(_)) if caller.same != body.same => {
            return Err(LowerError::SameConflictsWithBody {
                product: caller.product.clone(),
            })
        }
        (None, Some(same)) => merged.same = Some(same.clone()),
        _ => {}
    }
    match (&caller.selection, &body.selection) {
        (Some(given), Some(used)) if given != used => {
            return Err(LowerError::SelectionConflictsWithBody {
                product: caller.product.clone(),
                given: format!("@{}({})", given.keyword(), given.dimension()),
                used: format!("@{}({})", used.keyword(), used.dimension()),
            });
        }
        (None, Some(selection)) => merged.selection = Some(selection.clone()),
        _ => {}
    }
    Ok(merged)
}

pub(super) fn check_bodies_have_no_commands(builder: &PipelineBuilder) -> Result<(), ParseError> {
    let PipelineBuilder {
        pipeline,
        lines,
        operation_at,
        ..
    } = builder;
    for (index, command) in pipeline.commands.iter().enumerate() {
        let Some(&operation) = operation_at.get(&command.operation) else {
            continue;
        };
        let Some(place) = lines.command(CommandId::at(index)) else {
            continue;
        };
        if !pipeline.operations[operation].steps.is_empty() {
            return Err(ParseError::of(
                place.line,
                LowerError::CommandOnOperationWithSteps {
                    operation: command.operation.to_string(),
                    keyword: match command.role {
                        CommandRole::Run
                            if command.lines.iter().any(|line| line.expression().is_some()) =>
                        {
                            "calculate"
                        }
                        CommandRole::Run => "command",
                        CommandRole::Verify => "verify",
                    }
                    .to_string(),
                },
            )
            .within(&place));
        }
    }
    Ok(())
}
