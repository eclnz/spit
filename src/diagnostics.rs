//! Editor-friendly validation of an in-memory SPIT document.

use std::path::Path;

use crate::{
    parse_document, parse_document_at, parse_source_inventory, resolve, DefinitionSubject,
    EntityBinding, InputBinding, ParseError, Pipeline, ResolveError, SourceInventory,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub source: &'static str,
    pub line: Option<usize>,
    pub message: String,
}

/// Collect independent syntax errors throughout the document, then check its
/// semantics once the document parses. An absent inventory still allows
/// declaration and invocation checks, but cannot validate concrete jobs.
pub fn diagnose(text: &str, source_text: Option<&str>) -> Vec<Diagnostic> {
    diagnose_with_parser(text, source_text, parse_document)
}

/// Diagnose a document with its location available for resolving imports.
pub fn diagnose_at(text: &str, source_text: Option<&str>, path: &Path) -> Vec<Diagnostic> {
    diagnose_with_parser(text, source_text, |text| parse_document_at(text, path))
}

fn diagnose_with_parser(
    text: &str,
    source_text: Option<&str>,
    parser: impl Fn(&str) -> Result<(Pipeline, Option<SourceInventory>), ParseError>,
) -> Vec<Diagnostic> {
    let (document, pipeline_errors) = recover_parse_errors(text, parser);
    let (external_inventory, inventory_errors) = if let Some(source_text) = source_text {
        recover_parse_errors(source_text, parse_source_inventory)
    } else {
        (None, Vec::new())
    };
    let mut diagnostics: Vec<_> = pipeline_errors
        .into_iter()
        .map(|error| Diagnostic {
            source: "pipeline",
            line: Some(error.line),
            message: error.message,
        })
        .chain(inventory_errors.into_iter().map(|error| Diagnostic {
            source: "inventory",
            line: Some(error.line),
            message: error.message,
        }))
        .collect();
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    let (pipeline, embedded_inventory) = document.expect("document parsed without errors");
    let inventory = external_inventory
        .or(embedded_inventory)
        .unwrap_or_else(SourceInventory::default);

    match resolve(&pipeline, &inventory) {
        Ok(_) => diagnostics,
        Err(error) => {
            let inventory_text = source_text.unwrap_or(text);
            let (source, line) =
                error_location(&pipeline, &error, inventory_text, source_text.is_some());
            diagnostics.push(Diagnostic {
                source,
                line,
                message: error.to_string(),
            });
            diagnostics
        }
    }
}

/// Blank only the line that failed, preserving all later line numbers. This
/// lets the existing parser continue to report errors on other lines without
/// changing the fail-fast parsing API used by the CLI and library callers.
fn recover_parse_errors<T>(
    text: &str,
    parse: impl Fn(&str) -> Result<T, ParseError>,
) -> (Option<T>, Vec<ParseError>) {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let original_lines = lines.clone();
    let mut errors = Vec::new();
    loop {
        match parse(&lines.join("\n")) {
            Ok(parsed) => return (Some(parsed), errors),
            Err(error) => {
                let Some(index) = error.line.checked_sub(1) else {
                    errors.push(error);
                    return (None, errors);
                };
                let Some(line) = lines.get_mut(index) else {
                    errors.push(error);
                    return (None, errors);
                };
                if line.trim().is_empty() {
                    errors.push(error);
                    return (None, errors);
                }
                line.clear();
                if !depends_on_invalid_operation(&error, &errors, &original_lines) {
                    errors.push(error);
                }
            }
        }
    }
}

fn depends_on_invalid_operation(
    error: &ParseError,
    previous_errors: &[ParseError],
    original_lines: &[String],
) -> bool {
    if !error
        .message
        .ends_with("must be declared before its first flow step")
    {
        return false;
    }
    let Some(operation) = error
        .message
        .strip_prefix("operation `")
        .and_then(|message| message.split_once('`'))
        .map(|(name, _)| name)
    else {
        return false;
    };
    previous_errors.iter().any(|previous| {
        original_lines
            .get(previous.line.saturating_sub(1))
            .and_then(|line| line.trim().strip_prefix("operation "))
            .is_some_and(|declaration| match declaration.split_once('(') {
                Some((name, _)) => name.trim() == operation,
                // Without `(` the name boundary is unknown, so accept a prefix.
                None => declaration.starts_with(operation),
            })
    })
}

fn error_location(
    pipeline: &Pipeline,
    error: &ResolveError,
    inventory_text: &str,
    external_inventory: bool,
) -> (&'static str, Option<usize>) {
    let lines = &pipeline.source_lines;
    let invocation_line = |output: &str| lines.invocations.get(output).copied();
    let unique_operation_line = |operation: &str| {
        let mut matching = pipeline
            .invocations
            .iter()
            .filter(|invocation| invocation.operation == operation)
            .filter_map(|invocation| invocation_line(&invocation.output_product));
        let first = matching.next()?;
        matching.next().is_none().then_some(first)
    };
    let pipeline_line = match error {
        ResolveError::TypeMismatch { output_product, .. }
        | ResolveError::TypeVariableConflict { output_product, .. }
        | ResolveError::MissingInput { output_product, .. } => invocation_line(output_product),
        ResolveError::UnknownOperation { name } => unique_operation_line(name),
        ResolveError::UnknownProduct { name } => pipeline
            .invocations
            .iter()
            .find(|invocation| {
                invocation
                    .inputs
                    .iter()
                    .any(|binding| binding.product_name() == name)
            })
            .and_then(|invocation| invocation_line(&invocation.output_product))
            .or_else(|| lines.constraints.get(name).copied()),
        ResolveError::InvalidAggregationDimension { product, dimension } => pipeline
            .invocations
            .iter()
            .find(|invocation| {
                invocation.inputs.iter().any(|binding| {
                    matches!(binding, InputBinding::Vary { product: name, dimension: axis }
                        if name == product && axis == dimension)
                })
            })
            .and_then(|invocation| invocation_line(&invocation.output_product)),
        ResolveError::Cycle { products } => products.first().and_then(|name| invocation_line(name)),
        ResolveError::UnsupportedShapeRelationship { operation, .. } => {
            unique_operation_line(operation)
        }
        ResolveError::CoverageViolation { rule_index, .. } => {
            lines.constraint_lines.get(*rule_index).copied()
        }
        ResolveError::DuplicateOutputArtifact { artifact } => invocation_line(&artifact.product),
        ResolveError::InvalidDefinition { subject, .. } => match subject {
            DefinitionSubject::Product(name) => lines.products.get(name).copied(),
            DefinitionSubject::Operation(name) => lines.operations.get(name).copied(),
            DefinitionSubject::Invocation(output) => invocation_line(output),
            DefinitionSubject::Constraint(index) => lines.constraint_lines.get(*index).copied(),
            DefinitionSubject::Source(_) | DefinitionSubject::None => None,
        },
        ResolveError::DuplicateSourceArtifact { .. } => None,
    };
    if pipeline_line.is_some() {
        return ("pipeline", pipeline_line);
    }

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
    if inventory_line.is_some() {
        (
            if external_inventory {
                "inventory"
            } else {
                "pipeline"
            },
            inventory_line,
        )
    } else {
        ("pipeline", None)
    }
}

fn inventory_record_lines(
    text: &str,
    product: &str,
    entities: Option<&EntityBinding>,
) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let candidate = format!("sources:\n{line}\n");
            let parsed = parse_source_inventory(&candidate).ok()?;
            let record = parsed.artifacts.first()?;
            (record.product == product && entities.is_none_or(|value| value == &record.entities))
                .then_some(index + 1)
        })
        .collect()
}
