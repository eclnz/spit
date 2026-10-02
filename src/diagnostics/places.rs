//! Where in the text an error is: the part of a declaration, step, rule or
//! inventory record it is about.

use crate::model::DEFAULT_OUTPUT;
use crate::parser::{source_record_lines, Rule, SourceMap, Step};
use crate::span::{content_columns, Place};
use crate::{DefinitionSubject, EntityBinding, Pipeline, ResolveError};

use super::DiagnosticSource;

/// The part of a declaration, step, or rule that a pipeline error is about.
pub(super) fn subject_place(
    pipeline: &Pipeline,
    lines: &SourceMap,
    subject: &DefinitionSubject,
    error: &ResolveError,
) -> Option<Place> {
    match subject {
        DefinitionSubject::Product(name) => lines.products.get(name).cloned(),
        DefinitionSubject::Operation(name) => lines.operations.get(name).cloned(),
        DefinitionSubject::Invocation(output) => {
            let step = lines.invocations.get(output)?;
            Some(step_part(pipeline, step, output, error))
        }
        // A rule's own errors concern its product: unknown, or not a source.
        DefinitionSubject::Constraint(index) => lines.rules.get(*index).map(Rule::product),
        DefinitionSubject::ConstraintGroup(index) => lines.rules.get(*index).map(Rule::dimensions),
        DefinitionSubject::Exclusion(index) => lines.exclusions.get(*index).cloned(),
        DefinitionSubject::Stage(name) => lines.stages.get(name).cloned(),
        DefinitionSubject::Source(_) | DefinitionSubject::None => None,
    }
}

/// The part of the step producing `output` that `error` is about: an input,
/// the output, the operation name, or the whole call.
fn step_part(pipeline: &Pipeline, step: &Step, output: &str, error: &ResolveError) -> Place {
    let invocation = pipeline
        .invocations
        .iter()
        .find(|invocation| invocation.outputs.iter().any(|name| name == output));
    let input_named = |product: &str| {
        let index = invocation?
            .inputs
            .iter()
            .position(|binding| binding.product_name() == product)?;
        step.input(index)
    };
    let port = |port: &str| {
        let operation = invocation.and_then(|invocation| {
            pipeline
                .operations
                .iter()
                .find(|operation| operation.name == invocation.operation)
        });
        let Some(operation) = operation else {
            return (port == DEFAULT_OUTPUT).then(|| step.output());
        };
        if let Some(index) = operation
            .outputs
            .iter()
            .position(|output| output.name == port)
        {
            return Some(step.output_at(index));
        }
        let index = operation
            .inputs
            .iter()
            .position(|input| input.name == port)?;
        step.input(index)
    };
    let part = match error {
        ResolveError::TypeMismatch { site, .. }
        | ResolveError::TypeVariableConflict { site, .. }
        | ResolveError::MissingInput { site, .. }
        | ResolveError::AmbiguousInput { site, .. }
        | ResolveError::CollectionTooSmall { site, .. } => port(&site.port),
        ResolveError::UnknownProduct { name } if name == output => Some(step.output()),
        ResolveError::UnknownProduct { name } => input_named(name),
        ResolveError::UnknownOperation { .. } => Some(step.operation()),
        ResolveError::InvalidAggregationDimension { product, .. } => input_named(product),
        ResolveError::UnsupportedShapeRelationship { .. } => Some(step.call()),
        _ => Some(step.output()),
    };
    part.unwrap_or_else(|| step.call())
}

pub(super) fn error_location(
    pipeline: &Pipeline,
    lines: &SourceMap,
    error: &ResolveError,
    inventory_text: &str,
    external_inventory: bool,
) -> (DiagnosticSource, Option<Place>) {
    if let Some(place) = pipeline_place(pipeline, lines, error) {
        return (DiagnosticSource::Pipeline, Some(place));
    }
    let inventory_place = inventory_place(error, inventory_text);
    match inventory_place {
        Some(_) if external_inventory => (DiagnosticSource::Inventory, inventory_place),
        _ => (DiagnosticSource::Pipeline, inventory_place),
    }
}

/// Where in the pipeline `error` is, when it is about a step, rule or
/// declaration there.
fn pipeline_place(pipeline: &Pipeline, lines: &SourceMap, error: &ResolveError) -> Option<Place> {
    // The part of the step producing `output` that `error` is about.
    let step = |output: &str| {
        let step = lines.invocations.get(output)?;
        Some(step_part(pipeline, step, output, error))
    };
    let unique_step = |operation: &str| {
        let mut matching = pipeline
            .invocations
            .iter()
            .filter(|invocation| invocation.operation == operation)
            .filter_map(|invocation| step(invocation.output_product()));
        let first = matching.next()?;
        matching.next().is_none().then_some(first)
    };
    match error {
        ResolveError::TypeMismatch { site, .. }
        | ResolveError::TypeVariableConflict { site, .. }
        | ResolveError::MissingInput { site, .. }
        | ResolveError::AmbiguousInput { site, .. }
        | ResolveError::CollectionTooSmall { site, .. } => step(&site.output_product),
        ResolveError::UnknownOperation { name } => unique_step(name),
        ResolveError::UnknownProduct { name } => pipeline
            .invocations
            .iter()
            .find(|invocation| {
                invocation
                    .inputs
                    .iter()
                    .any(|binding| binding.product_name() == name)
            })
            .and_then(|invocation| step(invocation.output_product()))
            .or_else(|| lines.constraints.get(name).map(Rule::product)),
        ResolveError::InvalidAggregationDimension { product, dimension } => pipeline
            .invocations
            .iter()
            .find(|invocation| {
                invocation
                    .inputs
                    .iter()
                    .any(|binding| &binding.product == product && binding.vary.contains(dimension))
            })
            .and_then(|invocation| step(invocation.output_product())),
        ResolveError::Cycle { products } => products.first().and_then(|name| step(name)),
        ResolveError::UnsupportedShapeRelationship { operation, .. } => unique_step(operation),
        // Too few or too many artifacts is about the rule as a whole.
        ResolveError::CoverageViolation { rule_index, .. }
        | ResolveError::MissingRequiredValue { rule_index, .. }
        | ResolveError::NoGroupsToCheck { rule_index, .. } => {
            lines.rules.get(*rule_index).map(Rule::whole)
        }
        ResolveError::DuplicateOutputArtifact { artifact } => step(&artifact.product),
        ResolveError::InvalidDefinition { subject, .. } => {
            subject_place(pipeline, lines, subject, error)
        }
        ResolveError::DuplicateSourceArtifact { .. } => None,
    }
}

/// The inventory record `error` is about, when it is about one: the whole
/// line.
fn inventory_place(error: &ResolveError, inventory_text: &str) -> Option<Place> {
    let inventory_line = match error {
        ResolveError::UnknownProduct { name } => inventory_record_lines(inventory_text, name, None)
            .into_iter()
            .next(),
        ResolveError::DuplicateSourceArtifact { artifact } => {
            inventory_record_lines(inventory_text, &artifact.product, Some(&artifact.entities))
                .into_iter()
                .nth(1)
        }
        ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Source(record),
            ..
        } => inventory_record_lines(inventory_text, &record.product, Some(&record.entities))
            .into_iter()
            .next(),
        _ => None,
    };
    // A source record is one line; point at all of it.
    inventory_line.and_then(|line| {
        let text = inventory_text.lines().nth(line.checked_sub(1)?)?;
        Some(Place::new(line, content_columns(text)))
    })
}

fn inventory_record_lines(
    text: &str,
    product: &str,
    entities: Option<&EntityBinding>,
) -> Vec<usize> {
    source_record_lines(text)
        .into_iter()
        .filter_map(|(line, record)| {
            (record.product == product && entities.is_none_or(|value| value == &record.entities))
                .then_some(line)
        })
        .collect()
}
