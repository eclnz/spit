//! Editor-friendly validation of an in-memory SPIT document.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::bash::{collect_commands, collect_paths};
use crate::resolver::{collect_pipeline, Site};
use crate::{
    parse_document, parse_document_at, parse_source_inventory, resolve, InputBinding, ParseError,
    Pipeline, ResolveError, SourceInventory,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// `"pipeline"`, or `"inventory"` for a separate inventory file.
    pub source: &'static str,
    pub line: Option<usize>,
    pub message: String,
}

impl Diagnostic {
    fn error(source: &'static str, line: Option<usize>, message: String) -> Self {
        Self {
            severity: Severity::Error,
            source,
            line,
            message,
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Reads as `error: line 3: message`, naming the inventory for its lines.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.severity.as_str())?;
        match (self.source, self.line) {
            ("inventory", Some(line)) => write!(f, "inventory line {line}: ")?,
            ("inventory", None) => f.write_str("inventory: ")?,
            (_, Some(line)) => write!(f, "line {line}: ")?,
            (_, None) => {}
        }
        f.write_str(&self.message)
    }
}

/// Collect independent syntax errors throughout the document, then check its
/// semantics once the document parses: every declaration, step, rule,
/// command, and path error, and warnings for likely mistakes. Jobs are
/// resolved against the inventory only once nothing else is wrong. An absent
/// inventory still allows every other check. Diagnostics are ordered by
/// line, with at most one error per line.
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
        .map(|error| Diagnostic::error("pipeline", Some(error.line), error.message))
        .chain(
            inventory_errors
                .into_iter()
                .map(|error| Diagnostic::error("inventory", Some(error.line), error.message)),
        )
        .collect();
    if !diagnostics.is_empty() {
        return order(diagnostics);
    }

    let (pipeline, embedded_inventory) = document.expect("document parsed without errors");
    let inventory_text = source_text.unwrap_or(text);
    diagnostics.extend(pipeline_diagnostics(&pipeline, inventory_text));
    if diagnostics.iter().any(Diagnostic::is_error) {
        return order(diagnostics);
    }
    let inventory = external_inventory
        .or(embedded_inventory)
        .unwrap_or_else(SourceInventory::default);
    if let Err(error) = resolve(&pipeline, &inventory) {
        let (source, line) =
            error_location(&pipeline, &error, inventory_text, source_text.is_some());
        diagnostics.push(Diagnostic::error(source, line, error.to_string()));
    }
    order(diagnostics)
}

/// Every error in the pipeline text, then warnings about names that did not
/// fail. Needs no inventory.
fn pipeline_diagnostics(pipeline: &Pipeline, inventory_text: &str) -> Vec<Diagnostic> {
    let checked = collect_pipeline(pipeline);
    let lines = &pipeline.source_lines;
    let mut diagnostics: Vec<_> = checked
        .errors
        .iter()
        .map(|(site, error)| {
            let line = match site {
                Site::Product(name) => lines.products.get(name).copied(),
                Site::Operation(name) => lines.operations.get(name).copied(),
                Site::Invocation(output) => lines.invocations.get(output).copied(),
                Site::Rule(index) => lines.constraint_lines.get(*index).copied(),
            }
            .or_else(|| error_location(pipeline, error, inventory_text, false).1);
            Diagnostic::error("pipeline", line, error.to_string())
        })
        .collect();
    let bash_errors = collect_commands(pipeline, &checked.poisoned)
        .into_iter()
        .chain(collect_paths(pipeline, &checked.poisoned).1);
    for error in bash_errors {
        diagnostics.push(Diagnostic::error("pipeline", error.line, error.message));
    }
    diagnostics.extend(warnings(pipeline, &checked.poisoned));
    diagnostics
}

/// Order by source and line, keep only the first error on each line, and
/// drop warnings on a line that already has an error.
fn order(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let key = |diagnostic: &Diagnostic| {
        (
            diagnostic.source == "inventory",
            diagnostic.line.is_none(),
            diagnostic.line,
        )
    };
    diagnostics.sort_by_key(|diagnostic| (key(diagnostic), diagnostic.severity));
    let mut errored = BTreeSet::new();
    diagnostics.retain(|diagnostic| {
        let Some(line) = diagnostic.line else {
            return true;
        };
        let place = (diagnostic.source, line);
        if errored.contains(&place) {
            return false;
        }
        if diagnostic.is_error() {
            errored.insert(place);
        }
        true
    });
    diagnostics
}

/// Legal but likely mistaken pipeline text. Names in `skip` already have an
/// error. Nothing is reported as unused in a file with no steps, which is a
/// library of definitions, nor for imported names: a library is imported for
/// the definitions a pipeline needs, and is linted on its own.
fn warnings(pipeline: &Pipeline, skip: &BTreeSet<String>) -> Vec<Diagnostic> {
    let lines = &pipeline.source_lines;
    let warn = |line, message| Diagnostic {
        severity: Severity::Warning,
        source: "pipeline",
        line,
        message,
    };
    let used_operations: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .map(|invocation| invocation.operation.as_str())
        .collect();
    let outputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .map(|invocation| invocation.output_product.as_str())
        .collect();
    let inputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| invocation.inputs.iter().map(InputBinding::product_name))
        .collect();
    let commands: BTreeSet<_> = pipeline
        .commands
        .iter()
        .map(|command| command.operation.as_str())
        .collect();

    let library = pipeline.invocations.is_empty();
    let mut warnings = Vec::new();
    for product in &pipeline.products {
        let name = product.name.as_str();
        if library || skip.contains(name) || lines.imported.contains(name) {
            continue;
        }
        if !outputs.contains(name) && !inputs.contains(name) {
            warnings.push(warn(
                lines.products.get(name).copied(),
                format!("source product `{name}` is never used as an input"),
            ));
        }
    }
    for operation in &pipeline.operations {
        let name = operation.name.as_str();
        if skip.contains(name) {
            continue;
        }
        let line = lines.operations.get(name).copied();
        let imported = lines.imported.contains(name);
        if !used_operations.contains(name) {
            if !imported && !library {
                warnings.push(warn(
                    line,
                    format!("operation `{name}` is declared but never used"),
                ));
            }
        } else if !pipeline.commands.is_empty() && !commands.contains(name) {
            // Only once commands are in use: a pipeline may be written for its DAG alone.
            warnings.push(warn(
                line,
                format!("operation `{name}` has no command, so `bash` cannot run its jobs"),
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
        for variable in operation.output_type.variables().difference(&bound) {
            warnings.push(warn(
                line,
                format!(
                    "output type variable `{variable}` of `{name}` appears in no input; it is known only where the output product declares its type"
                ),
            ));
        }
    }
    warnings
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
            .is_some_and(|declaration| declaration.starts_with(operation))
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
        | ResolveError::MissingInput { output_product, .. }
        | ResolveError::AmbiguousInput { output_product, .. } => invocation_line(output_product),
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
        ResolveError::InvalidDefinition { detail } => {
            let name = detail.split('`').nth(1);
            name.and_then(|name| {
                if detail.starts_with("coverage rule") {
                    lines.constraints.get(name).copied()
                } else if detail.contains("producing invocation") {
                    invocation_line(name)
                } else if detail.starts_with("operation `")
                    || detail.starts_with("duplicate operation name")
                {
                    lines.operations.get(name).copied()
                } else if detail.starts_with("product `")
                    || detail.starts_with("duplicate product name")
                {
                    lines.products.get(name).copied()
                } else {
                    None
                }
            })
        }
    };
    if pipeline_line.is_some() {
        return ("pipeline", pipeline_line);
    }

    let inventory_line = match error {
        ResolveError::UnknownProduct { name } => inventory_record_lines(inventory_text, name, None)
            .into_iter()
            .next(),
        ResolveError::DuplicateOutputArtifact { artifact } => {
            inventory_record_lines(inventory_text, &artifact.product, Some(&artifact.entities))
                .into_iter()
                .nth(1)
        }
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
    entities: Option<&crate::EntityBinding>,
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
