//! Runtime checks: the checks on `check` declarations and on the uses ports
//! and sources make of them, and what each step's jobs run.
//!
//! A check reads one artifact, so it never adds a dependency or changes the
//! graph; it only becomes commands on the jobs that read or write that
//! artifact.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::command::CommandError;
use crate::model::{
    stage_and_parents, CheckDef, CheckUse, DefaultChecks, Invocation, OperationDef, Pipeline,
    PipelineIndex, Port,
};
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
            uses(&port.exempt, place.clone());
        }
    }
    uses_default(
        &mut uses,
        &pipeline.default_checks,
        lines.default_checks.clone(),
    );
    for stage in &pipeline.stages {
        uses_default(
            &mut uses,
            &stage.checks,
            lines.stage_checks.get(&stage.name).cloned(),
        );
    }
    for product in &pipeline.products {
        uses(&product.checks, lines.products.get(&product.name).cloned());
    }
    errors
}

/// Check the uses of one `check:` list, those it adds and those it drops.
fn uses_default(
    uses: &mut impl FnMut(&[CheckUse], Option<Place>),
    defaults: &DefaultChecks,
    place: Option<Place>,
) {
    uses(&defaults.checks, place.clone());
    uses(&defaults.exempt, place);
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

/// Definitions and producer checks found once for all the steps being bound.
/// The first declaration of a name wins, as the old binding search did.
pub(crate) struct CheckIndex<'p> {
    defaults: &'p DefaultChecks,
    definitions: FxHashMap<&'p str, &'p CheckDef>,
    stages: FxHashMap<&'p str, &'p DefaultChecks>,
    produced: Vec<Option<Vec<&'p CheckUse>>>,
}

impl<'p> CheckIndex<'p> {
    pub(crate) fn new(index: &PipelineIndex<'p>) -> Self {
        let pipeline = index.pipeline;
        if pipeline.checks.is_empty() {
            return Self {
                defaults: &pipeline.default_checks,
                definitions: FxHashMap::default(),
                stages: FxHashMap::default(),
                produced: Vec::new(),
            };
        }
        let mut definitions = FxHashMap::default();
        for check in &pipeline.checks {
            definitions.entry(check.name.as_str()).or_insert(check);
        }
        let mut stages = FxHashMap::default();
        for stage in &pipeline.stages {
            stages.entry(stage.name.as_str()).or_insert(&stage.checks);
        }
        let mut result = Self {
            defaults: &pipeline.default_checks,
            definitions,
            stages,
            produced: vec![None; pipeline.products.len()],
        };
        for (number, product) in pipeline.products.iter().enumerate() {
            let Some((invocation, port)) = index.producer(&product.name) else {
                continue;
            };
            let Some(operation) = index.operation(&invocation.operation) else {
                continue;
            };
            result.produced[number] = Some(result.output_checks(operation, Some(invocation), port));
        }
        result
    }

    fn produced(&self, index: &PipelineIndex<'_>, product: &str) -> Option<&[&'p CheckUse]> {
        let id = index.id(product)?;
        self.produced[id.index()].as_deref()
    }

    /// Checks on one output, including defaults inherited from enclosing stages.
    fn output_checks(
        &self,
        operation: &'p OperationDef,
        invocation: Option<&'p Invocation>,
        port: usize,
    ) -> Vec<&'p CheckUse> {
        let output = &operation.outputs[port];
        let mut checks: Vec<&CheckUse> = Vec::new();
        let mut apply = |defaults: &'p DefaultChecks| {
            for used in &defaults.checks {
                if !checks.contains(&used) {
                    checks.push(used);
                }
            }
            checks.retain(|used| !defaults.exempt.contains(used));
        };
        apply(self.defaults);
        if let Some(stage) = invocation.and_then(|invocation| invocation.stage.as_deref()) {
            let mut around: Vec<&str> = stage_and_parents(stage).collect();
            around.reverse();
            for name in around {
                if let Some(defaults) = self.stages.get(name) {
                    apply(defaults);
                }
            }
        }
        checks.retain(|used| !output.exempt.contains(used));
        let added = invocation
            .map_or(&[][..], |invocation| invocation.checks.as_slice())
            .iter()
            .filter(|(at, _)| *at == Port::Output(port))
            .map(|(_, check)| check);
        checks.extend(output.checks.iter().chain(added));
        checks
    }
}

/// The checks the jobs of `invocation`'s step run, in order: on inputs,
/// then outputs, by port, each port's in the order they are attached. An
/// input runs its port's checks and its source's. Every check use names a
/// declared check, which [`collect_checks`] makes sure of.
pub(crate) fn step_checks<'p>(
    index: &PipelineIndex<'p>,
    found: &CheckIndex<'p>,
    operation: &'p OperationDef,
    invocation: Option<&'p Invocation>,
) -> Vec<StepCheck<'p>> {
    if found.definitions.is_empty() {
        return Vec::new();
    }
    let find = |name: &str| {
        found
            .definitions
            .get(name)
            .copied()
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
        let produced = product.and_then(|name| found.produced(index, name));
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
                covered: produced.is_some_and(|checks| checks.contains(&used)),
            });
        }
    }
    for port in 0..operation.outputs.len() {
        let mut seen: Vec<&CheckUse> = Vec::new();
        let output = invocation
            .and_then(|invocation| invocation.outputs.get(port))
            .and_then(|product| found.produced(index, product));
        let output_checks;
        let used_checks = match output {
            Some(checks) => checks,
            None => {
                output_checks = found.output_checks(operation, invocation, port);
                &output_checks
            }
        };
        for &used in used_checks {
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{step_checks, CheckIndex};
    use crate::model::PipelineIndex;
    use crate::parse_pipeline;

    fn checked_steps(steps: usize) -> Duration {
        let mut text = String::from("source raw : T [sub]\n");
        for index in 0..steps {
            text += &format!("check c{index}: test -s {{@path}}\n");
        }
        text += &format!(
            "operation step(x: T @ check(c{})) -> T @ check(c{})\n",
            steps - 1,
            steps - 1
        );
        let mut previous = "raw".to_owned();
        for index in 0..steps {
            text += &format!(
                "stage s{index}:\n    check: c{}\n    p{index} = step({previous})\n",
                steps - 1
            );
            previous = format!("p{index}");
        }
        let pipeline = parse_pipeline(&text).unwrap();
        let index = PipelineIndex::new(&pipeline);
        let mut best = Duration::MAX;
        for _ in 0..3 {
            let start = Instant::now();
            let checks = CheckIndex::new(&index);
            for (number, invocation) in pipeline.invocations.iter().enumerate() {
                let operation = index.operation(&invocation.operation).unwrap();
                let bound = step_checks(&index, &checks, operation, Some(invocation));
                assert_eq!(bound.len(), 2);
                assert_eq!(bound[0].covered, number > 0);
            }
            best = best.min(start.elapsed());
        }
        best
    }

    #[test]
    fn checked_step_binding_scales_with_steps_and_definitions() {
        let small = checked_steps(2000);
        let large = checked_steps(8000);
        eprintln!("check binding: {small:?} for 2000, {large:?} for 8000 steps");
        assert!(
            large < Duration::from_millis(25) || large.as_secs_f64() < 9.0 * small.as_secs_f64(),
            "check binding grew faster than its steps and definitions"
        );
    }
}
