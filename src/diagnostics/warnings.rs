//! Warnings: what checks clean but is probably not what the author meant.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    stage_within, ArtifactReport, CommandRole, PipelineIndex, ResolvedDag, SourceInventory,
};
use crate::parser::{SourceMap, Step};
use crate::paths::{case_collisions, dashed_labels};
use crate::span::Place;
use crate::{InputBinding, Pipeline, ResolveError};

use super::{Diagnostic, DiagnosticSource, Severity};

/// Unused sources that spell a value nearly as an incomplete job needs it.
pub(super) fn near_miss_warnings(report: &ArtifactReport) -> Vec<Diagnostic> {
    let unused: BTreeSet<_> = report
        .unused_sources()
        .into_iter()
        .map(|id| report.dag.artifact(id).key())
        .collect();
    let mut seen = BTreeSet::new();
    let mut warnings = Vec::new();
    for job in &report.incomplete {
        for gap in &job.gaps {
            let crate::model::Gap::Unmatched(ResolveError::MissingInput {
                site,
                near: Some(near),
                ..
            }) = gap
            else {
                continue;
            };
            let key = near.artifact.view().key();
            if unused.contains(&key) && seen.insert(key) {
                warnings.push(warning(None, format!(
                    "source {} is used by no job; `{}` differs only in {} from {}, which `{}` needs",
                    near.artifact, near.dimension, near.reason, near.wanted, site.operation
                )));
            }
        }
    }
    warnings
}

/// Flag paths that differ only in case, which are one file on macOS and Windows.
pub(super) fn case_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    dag: &ResolvedDag,
) -> Vec<Diagnostic> {
    let index = PipelineIndex::new(pipeline);
    case_collisions(pipeline, dag)
        .into_iter()
        .map(|[(first, first_path), (second, second_path)]| {
            warning(
                lines.path_rule(&index, &second.0),
                format!(
                    "`{}[{}]` and `{}[{}]` have paths `{first_path}` and `{second_path}`, which differ only in case, so they are one file where case is ignored, as on macOS and Windows",
                    first.0, first.1, second.0, second.1
                ),
            )
        })
        .collect()
}

/// Flag values with a `-` that `{@labels}` writes, as `sub-01-a`, which
/// BIDS reads as a new entity.
pub(super) fn label_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    dag: &ResolvedDag,
) -> Vec<Diagnostic> {
    let index = PipelineIndex::new(pipeline);
    dashed_labels(pipeline, dag)
        .into_iter()
        .map(|(product, dimension, value)| {
            warning(
                lines.path_rule(&index, &product),
                format!(
                    "`{{@labels}}` writes `{dimension}-{value}` in the path of `{product}`; BIDS reads each `-` as the end of a key, so it cannot read `{value}` back"
                ),
            )
        })
        .collect()
}

/// Flag unquoted shell operators such as `>` or `|`, which SPIT passes to
/// the program as arguments.
pub(super) fn operator_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    text: &str,
) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    for (index, command) in pipeline.commands.iter().enumerate() {
        if lines.imported.contains(&command.operation) {
            continue;
        }
        let place = lines.command(index);
        let line_text = place
            .as_ref()
            .and_then(|place| text.lines().nth(place.line.checked_sub(1)?));
        for operator in command.template.shell_operators() {
            let columns = place.as_ref().zip(line_text).and_then(|(place, line)| {
                let start = place.columns.start;
                let region = line.get(start..place.columns.end)?;
                region
                    .match_indices(operator.as_str())
                    .find(|(at, _)| {
                        let before = region[..*at].chars().next_back();
                        let after = region[at + operator.len()..].chars().next();
                        before.is_none_or(char::is_whitespace)
                            && after.is_none_or(char::is_whitespace)
                    })
                    .map(|(at, _)| start + at..start + at + operator.len())
            });
            warnings.push(warning(
                place.as_ref().map(|place| {
                    Place::new(place.line, columns.unwrap_or_else(|| place.columns.clone()))
                }),
                format!(
                    "`{operator}` in the command for `{}` is passed to the program as an argument, not read as a pipe or redirection, since commands do not run through a shell; quote it to pass it on purpose",
                    command.operation
                ),
            ));
        }
    }
    warnings
}

/// Legal but likely mistaken pipeline text. Names in `skip` already have an
/// error. Nothing is reported as unused in a file with no steps, which is a
/// library of definitions, nor for imported names: a library is imported for
/// the definitions a pipeline needs, and is linted on its own.
pub(super) fn warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    // A library, with no steps, may declare what it never uses.
    if pipeline.invocations.is_empty() {
        return operation_warnings(pipeline, lines, skip, true);
    }
    let mut warnings = stage_warnings(pipeline, lines);
    warnings.extend(name_warnings(pipeline, lines));
    warnings.extend(product_warnings(pipeline, lines, skip));
    warnings.extend(operation_warnings(pipeline, lines, skip, false));
    warnings
}

fn warning(place: Option<Place>, message: String) -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        DiagnosticSource::Pipeline,
        place,
        message,
    )
}

/// Stages that hold no steps, directly or in stages nested in them.
fn stage_warnings(pipeline: &Pipeline, lines: &SourceMap) -> Vec<Diagnostic> {
    pipeline
        .stages
        .iter()
        .filter(|stage| {
            !pipeline.invocations.iter().any(|invocation| {
                invocation
                    .stage
                    .as_deref()
                    .is_some_and(|name| stage_within(name, &stage.name))
            })
        })
        .map(|stage| {
            warning(
                lines.stages.get(&stage.name).cloned(),
                format!("stage `{}` has no steps", stage.name),
            )
        })
        .collect()
}

/// Products named after the operation that makes them, as in
/// `digest = digest(log)`: legal, since the names are separate, but the step
/// then reads as the operation, not its result.
fn name_warnings(pipeline: &Pipeline, lines: &SourceMap) -> Vec<Diagnostic> {
    pipeline
        .invocations
        .iter()
        .flat_map(|invocation| {
            invocation
                .outputs
                .iter()
                .filter(move |output| **output == invocation.operation)
        })
        .map(|output| {
            warning(
                lines.invocations.get(output).map(Step::output),
                format!(
                    "product `{output}` has the name of the operation that makes it; \
                     name the result instead, so the step reads as what it makes"
                ),
            )
        })
        .collect()
}

/// Source products that no step reads.
fn product_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<Diagnostic> {
    let used: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| {
            invocation
                .outputs
                .iter()
                .map(String::as_str)
                .chain(invocation.inputs.iter().map(InputBinding::product_name))
        })
        .collect();
    pipeline
        .products
        .iter()
        .map(|product| product.name.as_str())
        .filter(|name| !skip.contains(*name) && !lines.imported.contains(*name))
        .filter(|name| !used.contains(name))
        .map(|name| {
            warning(
                lines.products.get(name).cloned(),
                format!("source product `{name}` is never used as an input"),
            )
        })
        .collect()
}

/// Operations that no step uses, that have no command while others do, or
/// whose output types name a variable no input binds.
fn operation_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
    library: bool,
) -> Vec<Diagnostic> {
    let used: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .map(|invocation| invocation.operation.as_str())
        .collect();
    let commands: BTreeSet<_> = pipeline
        .commands
        .iter()
        .filter(|command| command.role == CommandRole::Run)
        .map(|command| command.operation.as_str())
        .collect();
    let mut warnings = Vec::new();
    for operation in &pipeline.operations {
        let name = operation.name.as_str();
        if skip.contains(name) {
            continue;
        }
        let place = lines.operations.get(name).cloned();
        let imported = lines.imported.contains(name);
        if !used.contains(name) {
            if !imported && !library {
                warnings.push(warning(
                    place.clone(),
                    format!("operation `{name}` is declared but never used"),
                ));
            }
        } else if !pipeline.commands.is_empty() && !commands.contains(name) {
            // Only once commands are in use: a pipeline may be written for its DAG alone.
            warnings.push(warning(
                place.clone(),
                format!("operation `{name}` has no command, so its jobs cannot run"),
            ));
        }
        if imported {
            continue;
        }
        let bound: BTreeSet<_> = operation
            .inputs
            .iter()
            .flat_map(|port| port.artifact_type.variables())
            .collect();
        let produced: BTreeSet<_> = operation
            .outputs
            .iter()
            .flat_map(|port| port.artifact_type.variables())
            .collect();
        for variable in produced.difference(&bound) {
            warnings.push(warning(
                place.clone(),
                format!(
                    "output type variable `{variable}` of `{name}` appears in no input; it is known only where the output product declares its type"
                ),
            ));
        }
    }
    warnings
}

/// Steps that resolve no jobs from a supplied inventory. A source with no
/// artifacts is reported once, naming the steps it leaves empty; any other
/// step that is empty although its inputs are not is reported on its own.
/// For each product, the sources it depends on that have no artifacts.
/// Each product's set is found once, from its inputs' sets, so a step that
/// reads the same product twice costs no more than one that reads it once.
fn unobserved_sources<'a>(
    producers: &BTreeMap<&'a str, &'a crate::Invocation>,
    observed: &BTreeSet<&str>,
) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
    let mut found: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let products = producers
        .values()
        .flat_map(|invocation| &invocation.inputs)
        .map(|input| input.product.as_str());
    for product in products {
        // Depth first with an explicit stack: a product is finished once
        // every product it reads is.
        let mut stack = vec![(product, false)];
        while let Some((name, inputs_done)) = stack.pop() {
            if found.contains_key(name) {
                continue;
            }
            let Some(invocation) = producers.get(name) else {
                let sources = (!observed.contains(name)).then_some(name);
                found.insert(name, sources.into_iter().collect());
                continue;
            };
            let inputs = invocation.inputs.iter().map(|input| input.product.as_str());
            if inputs_done {
                let sources = inputs
                    .flat_map(|input| found.get(input).into_iter().flatten())
                    .copied()
                    .collect();
                found.insert(name, sources);
            } else {
                stack.push((name, true));
                stack.extend(inputs.map(|input| (input, false)));
            }
        }
    }
    found
}

pub(super) fn empty_step_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    produced: &BTreeSet<&str>,
    inventory: &SourceInventory,
) -> Vec<Diagnostic> {
    let observed: BTreeSet<_> = inventory
        .artifacts
        .iter()
        .map(|record| record.product.as_str())
        .collect();
    let producers: BTreeMap<_, _> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| {
            invocation
                .outputs
                .iter()
                .map(move |output| (output.as_str(), invocation))
        })
        .collect();
    let empty_input = |name: &str| match producers.get(name) {
        Some(producer) => !produced.contains(producer.output_product()),
        None => !observed.contains(name),
    };
    let unobserved = unobserved_sources(&producers, &observed);
    let mut left_empty: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut warnings = Vec::new();
    for invocation in &pipeline.invocations {
        let step = invocation.output_product();
        if produced.contains(step) {
            continue;
        }
        let sources: BTreeSet<_> = invocation
            .inputs
            .iter()
            .flat_map(|input| unobserved.get(input.product.as_str()).into_iter().flatten())
            .copied()
            .collect();
        for source in &sources {
            left_empty.entry(source).or_default().push(step);
        }
        if sources.is_empty()
            && !invocation
                .inputs
                .iter()
                .any(|input| empty_input(&input.product))
        {
            warnings.push(Diagnostic::new(
                Severity::Warning,
                DiagnosticSource::Pipeline,
                lines.invocations.get(step).map(Step::output),
                format!("`{step}` resolves no jobs: its inputs have artifacts, but none match each other or the step's selectors"),
            ));
        }
    }
    for (source, steps) in left_empty {
        warnings.push(Diagnostic::new(
            Severity::Warning,
            DiagnosticSource::Pipeline,
            lines.products.get(source).cloned(),
            format!(
                "source `{source}` has no artifacts in the inventory, so these steps resolve no jobs: {}",
                steps.join(", ")
            ),
        ));
    }
    warnings
}
