//! Editor-friendly validation of an in-memory SPIT document.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::imports::parse_located_document;
use crate::parser::{parse_document_with_imports, ParsedDocument, SourceMap};
use crate::{
    parse_source_inventory, resolve, DefinitionSubject, EntityBinding, InputBinding, ParseError,
    ParseErrorKind, Pipeline, ResolveError,
};

/// Which input text a diagnostic refers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSource {
    Pipeline,
    Inventory,
}

impl DiagnosticSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pipeline => "pipeline",
            Self::Inventory => "inventory",
        }
    }
}

impl fmt::Display for DiagnosticSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub source: DiagnosticSource,
    pub line: Option<usize>,
    pub message: String,
}

/// Collect independent syntax errors throughout the document, then check its
/// semantics once the document parses. An absent inventory still allows
/// declaration and invocation checks, but cannot validate concrete jobs.
pub fn diagnose(text: &str, source_text: Option<&str>) -> Vec<Diagnostic> {
    diagnose_with_parser(text, source_text, |text| {
        parse_document_with_imports(text, &BTreeMap::new())
    })
}

/// Diagnose a document with its location available for resolving imports.
pub fn diagnose_at(text: &str, source_text: Option<&str>, path: &Path) -> Vec<Diagnostic> {
    diagnose_with_parser(text, source_text, |text| parse_located_document(text, path))
}

fn diagnose_with_parser(
    text: &str,
    source_text: Option<&str>,
    parser: impl Fn(&str) -> Result<ParsedDocument, ParseError>,
) -> Vec<Diagnostic> {
    let (document, pipeline_errors) = recover_parse_errors(text, parser);
    let (external_inventory, inventory_errors) = source_text.map_or_else(
        || (None, Vec::new()),
        |source_text| recover_parse_errors(source_text, parse_source_inventory),
    );
    let diagnostics: Vec<_> = pipeline_errors
        .into_iter()
        .map(|error| (DiagnosticSource::Pipeline, error))
        .chain(
            inventory_errors
                .into_iter()
                .map(|error| (DiagnosticSource::Inventory, error)),
        )
        .map(|(source, error)| Diagnostic {
            source,
            line: Some(error.line),
            message: error.message,
        })
        .collect();
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    let document = document.expect("document parsed without errors");
    let inventory = external_inventory
        .or(document.inventory)
        .unwrap_or_default();

    match resolve(&document.pipeline, &inventory) {
        Ok(_) => Vec::new(),
        Err(error) => {
            let inventory_text = source_text.unwrap_or(text);
            let (source, line) = error_location(
                &document.pipeline,
                &document.lines,
                &error,
                inventory_text,
                source_text.is_some(),
            );
            vec![Diagnostic {
                source,
                line,
                message: error.to_string(),
            }]
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
                let Some(line) = error
                    .line
                    .checked_sub(1)
                    .and_then(|index| lines.get_mut(index))
                    .filter(|line| !line.trim().is_empty())
                else {
                    errors.push(error);
                    return (None, errors);
                };
                line.clear();
                if !depends_on_invalid_operation(&error, &errors, &original_lines) {
                    errors.push(error);
                }
            }
        }
    }
}

/// A call to an operation whose own declaration already failed to parse would
/// only repeat that error, so it is not reported separately.
fn depends_on_invalid_operation(
    error: &ParseError,
    previous_errors: &[ParseError],
    original_lines: &[String],
) -> bool {
    let ParseErrorKind::UndeclaredOperation { name: operation } = &error.kind else {
        return false;
    };
    previous_errors.iter().any(|previous| {
        original_lines
            .get(previous.line.saturating_sub(1))
            .and_then(|line| line.trim().strip_prefix("operation "))
            .is_some_and(|declaration| match declaration.split_once('(') {
                Some((name, _)) => name.trim() == operation,
                // Without `(` the name boundary is unknown, so accept a prefix.
                None => declaration.starts_with(operation.as_str()),
            })
    })
}

fn error_location(
    pipeline: &Pipeline,
    lines: &SourceMap,
    error: &ResolveError,
    inventory_text: &str,
    external_inventory: bool,
) -> (DiagnosticSource, Option<usize>) {
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
        return (DiagnosticSource::Pipeline, pipeline_line);
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
    match inventory_line {
        Some(_) if external_inventory => (DiagnosticSource::Inventory, inventory_line),
        _ => (DiagnosticSource::Pipeline, inventory_line),
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
