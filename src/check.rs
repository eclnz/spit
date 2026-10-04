//! Runtime checks: the checks on `check` declarations and on the uses ports
//! and sources make of them, and what each step's jobs run.
//!
//! A check reads one artifact, so it never adds a dependency or changes the
//! graph; it only becomes commands on the jobs that read or write that
//! artifact.

use std::collections::BTreeMap;

use crate::command::CommandError;
use crate::model::{CheckDef, CheckUse, Invocation, OperationDef, Pipeline, PipelineIndex, Port};
use crate::parser::SourceMap;
use crate::span::Place;
use crate::template::Part;

/// The placeholder a check's command reads its artifact from.
pub(crate) const CHECKED_PATH: &str = "@path";

/// Check every `check` declaration and every use of one, collecting each
/// error. Uses on operations in `skip`, which already failed, are not
/// checked.
pub(crate) fn collect_checks(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &std::collections::BTreeSet<String>,
) -> Vec<CommandError> {
    let mut errors = Vec::new();
    let mut checks: BTreeMap<&str, &CheckDef> = BTreeMap::new();
    for (index, check) in pipeline.checks.iter().enumerate() {
        let place = lines.checks.get(index).cloned();
        if checks.insert(&check.name, check).is_some() {
            errors.push(
                CommandError::new(format!("duplicate check `{}`", check.name))
                    .at(place)
                    .focus(&check.name),
            );
            continue;
        }
        if let Err(error) = check_template(check) {
            errors.push(error.at(place));
        }
    }
    let mut uses = |checked: &[CheckUse], place: Option<Place>| {
        for used in checked {
            let problem = match checks.get(used.check.as_str()) {
                None => format!(
                    "unknown check `{}`; declare it with `check {}: ...`",
                    used.check, used.check
                ),
                Some(check) if check.parameters.is_empty() && !used.arguments.is_empty() => {
                    format!(
                        "check `{}` takes no arguments; write `{}`",
                        check.name, used.check
                    )
                }
                Some(check) if check.parameters.len() != used.arguments.len() => format!(
                    "check `{}` takes {} ({}), but `{used}` gives {}",
                    check.name,
                    count(check.parameters.len(), "argument"),
                    check.parameters.join(", "),
                    used.arguments.len()
                ),
                Some(_) => continue,
            };
            errors.push(
                CommandError::new(problem)
                    .at(place.clone())
                    .focus(&used.check),
            );
        }
    };
    for operation in &pipeline.operations {
        if skip.contains(&operation.name) {
            continue;
        }
        let place = lines.operations.get(&operation.name).cloned();
        for port in &operation.inputs {
            uses(&port.checks, place.clone());
        }
        for port in &operation.outputs {
            uses(&port.checks, place.clone());
        }
    }
    for product in &pipeline.products {
        uses(&product.checks, lines.products.get(&product.name).cloned());
    }
    errors
}

fn count(number: usize, noun: &str) -> String {
    match number {
        1 => format!("1 {noun}"),
        _ => format!("{number} {noun}s"),
    }
}

/// A check's command may read `{@path}`, which it must, and its parameters,
/// each of which it must use.
fn check_template(check: &CheckDef) -> Result<(), CommandError> {
    let mut path = false;
    let mut used = vec![false; check.parameters.len()];
    for part in check.template.arguments().iter().flatten() {
        let Part::Placeholder(name) = part else {
            continue;
        };
        if name == CHECKED_PATH {
            path = true;
        } else if let Some(index) = check.parameters.iter().position(|p| p == name) {
            used[index] = true;
        } else {
            let can = if check.parameters.is_empty() {
                String::new()
            } else {
                format!(", and its parameters ({})", check.parameters.join(", "))
            };
            return Err(CommandError::new(format!(
                "check `{}` uses unknown placeholder `{{{name}}}`; a check reads only `{{@path}}`, the artifact it checks{can}",
                check.name
            ))
            .focus(format!("{{{name}}}")));
        }
    }
    if !path {
        return Err(CommandError::new(format!(
            "check `{}` must use `{{@path}}`, the artifact it checks",
            check.name
        ))
        .focus(&check.name));
    }
    if let Some(index) = used.iter().position(|used| !used) {
        let parameter = &check.parameters[index];
        return Err(CommandError::new(format!(
            "check `{}` never uses its parameter `{{{parameter}}}`",
            check.name
        ))
        .focus(&check.name));
    }
    Ok(())
}

/// When a check runs: before the job's command, on an input, or after it,
/// on an output.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum When {
    Before,
    After,
}

impl When {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::After => "after",
        }
    }
}

/// A check that every job of a step runs on the artifacts of one port.
pub(crate) struct StepCheck<'p> {
    pub(crate) when: When,
    /// The input port, or for `After` the output port, by index.
    pub(crate) port: usize,
    pub(crate) check: &'p CheckDef,
    pub(crate) arguments: &'p [String],
    /// The check as written, as in `ndim(4)`.
    pub(crate) written: String,
    /// Whether the job that makes the input runs this same check on it, so
    /// that a job in the same DAG need not run it again.
    pub(crate) covered: bool,
}

/// The checks the jobs of `invocation`'s step run, in order: on inputs,
/// then outputs, by port, each port's in the order they are attached. An
/// input runs its port's checks and its source's. Every check use names a
/// declared check, which [`collect_checks`] makes sure of.
pub(crate) fn step_checks<'p>(
    index: &PipelineIndex<'p>,
    operation: &'p OperationDef,
    invocation: Option<&'p Invocation>,
) -> Vec<StepCheck<'p>> {
    let pipeline = index.pipeline;
    if pipeline.checks.is_empty() {
        return Vec::new();
    }
    let find = |name: &str| {
        pipeline
            .checks
            .iter()
            .find(|check| check.name == name)
            .expect("collect_checks makes sure every check use names a check")
    };
    // What the step adds to its operation's checks, on one port.
    let added = |at: Port| {
        invocation
            .map_or(&[][..], |invocation| invocation.checks.as_slice())
            .iter()
            .filter(move |(port, _)| *port == at)
            .map(|(_, check)| check)
    };
    let mut checks = Vec::new();
    for (port, input) in operation.inputs.iter().enumerate() {
        let product = invocation.and_then(|invocation| invocation.inputs.get(port));
        let product = product.map(|binding| binding.product_name());
        let source = product
            .and_then(|name| index.product(name))
            .map_or(&[][..], |product| product.checks.as_slice());
        let produced = product.and_then(|name| producer_checks(index, name));
        let mut seen: Vec<&CheckUse> = Vec::new();
        for used in input
            .checks
            .iter()
            .chain(added(Port::Input(port)))
            .chain(source)
        {
            if seen.contains(&used) {
                continue;
            }
            seen.push(used);
            checks.push(StepCheck {
                when: When::Before,
                port,
                check: find(&used.check),
                arguments: &used.arguments,
                written: used.to_string(),
                covered: produced
                    .as_ref()
                    .is_some_and(|checks| checks.contains(&used)),
            });
        }
    }
    for (port, output) in operation.outputs.iter().enumerate() {
        let mut seen: Vec<&CheckUse> = Vec::new();
        for used in output.checks.iter().chain(added(Port::Output(port))) {
            if seen.contains(&used) {
                continue;
            }
            seen.push(used);
            checks.push(StepCheck {
                when: When::After,
                port,
                check: find(&used.check),
                arguments: &used.arguments,
                written: used.to_string(),
                covered: false,
            });
        }
    }
    checks
}

/// The checks the step that makes `product` runs on it after its command:
/// its operation's on that output, and those the step adds.
fn producer_checks<'p>(index: &PipelineIndex<'p>, product: &str) -> Option<Vec<&'p CheckUse>> {
    let (invocation, port) = index.producer(product)?;
    let operation = index.operation(&invocation.operation)?;
    let added = invocation
        .checks
        .iter()
        .filter(|(at, _)| *at == Port::Output(port))
        .map(|(_, check)| check);
    Some(
        operation
            .outputs
            .get(port)?
            .checks
            .iter()
            .chain(added)
            .collect(),
    )
}
