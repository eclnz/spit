//! Warnings: what checks clean but is probably not what the author meant.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{ArtifactReport, PipelineIndex, ResolvedDag, SourceInventory};
use crate::parser::{SourceMap, Step};
use crate::paths::{bound_paths, case_collisions, misread_sources};
use crate::span::Place;
use crate::validate::PipelineWarning;
use crate::{Pipeline, ResolveError};

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

/// Flag paths that differ only in case, which are one file on macOS and
/// Windows, and source paths their rule reads back as other values. Each
/// artifact's path is bound once for both.
pub(super) fn path_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    dag: &ResolvedDag,
) -> Vec<Diagnostic> {
    let Ok(paths) = bound_paths(pipeline, dag) else {
        return Vec::new();
    };
    let index = PipelineIndex::new(pipeline);
    let misread = misread_sources(pipeline, dag, &paths).into_iter().map(|found| {
        let (product, entities) = &found.artifact;
        warning(
            lines.path_rule(&index, product),
            format!(
                "`{product}[{entities}]` has path `{}`, which its rule reads back as `{product}[{}]`, so its file would be found under those values; use a separator between placeholders that no value holds",
                found.path, found.read
            ),
        )
    });
    case_collisions(dag, &paths)
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
        .chain(misread)
        .collect()
}

/// `warning` as a diagnostic, at the declaration or step it is about.
pub(super) fn pipeline_warning(warning: &PipelineWarning, lines: &SourceMap) -> Diagnostic {
    let place = match warning {
        PipelineWarning::EmptyStage { stage } => lines.stages.get(*stage).cloned(),
        PipelineWarning::ProductNamedAfterOperation { product } => {
            lines.invocations.get(*product).map(Step::output)
        }
        PipelineWarning::UnusedSource { product } => lines.products.get(*product).cloned(),
        PipelineWarning::OperationOutsideStage { operation, .. }
        | PipelineWarning::UnusedOperation { operation }
        | PipelineWarning::NoCommand { operation }
        | PipelineWarning::UnboundOutputVariable { operation, .. } => {
            lines.operations.get(*operation).cloned()
        }
    };
    self::warning(place, warning.to_string())
}

fn warning(place: Option<Place>, message: String) -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        DiagnosticSource::Pipeline,
        place,
        message,
    )
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

/// Warn of each step that resolves no jobs though its inputs have
/// artifacts, and of each source with none that leaves steps empty. A step
/// that reads a product in `withheld`, whose artifacts a review keeps from
/// it, is not warned of: the review's notes say why it has no jobs.
pub(super) fn empty_step_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    produced: &BTreeSet<&str>,
    withheld: &BTreeSet<&str>,
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
            && !invocation.inputs.iter().any(|input| {
                empty_input(&input.product) || withheld.contains(input.product.as_str())
            })
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
