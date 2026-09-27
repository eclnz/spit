//! Editor-friendly validation of an in-memory SPIT document.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::path::Path;

use crate::bash::collect_commands;
use crate::imports::parse_located_document;
use crate::parser::{
    glued_comment, parse_document_with_imports, ParsedDocument, Rule, SourceMap, Step,
};
use crate::paths::collect_paths;
use crate::resolver::collect_pipeline;
use crate::span::{columns_of, content_columns, find_word, utf16_columns, Place};
use crate::{
    parse_source_inventory, resolve, DefinitionSubject, EntityBinding, InputBinding, ParseError,
    ParseErrorKind, Pipeline, ResolveError,
};

/// Which input text a diagnostic refers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
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
    pub source: DiagnosticSource,
    pub line: Option<usize>,
    /// The byte range within the line that the diagnostic is about. Every
    /// diagnostic with a line has one; at least the line's content.
    pub columns: Option<Range<usize>>,
    pub message: String,
}

impl Diagnostic {
    fn new(
        severity: Severity,
        source: DiagnosticSource,
        place: Option<Place>,
        message: String,
    ) -> Self {
        let (line, columns) = place.map_or((None, None), |place| {
            (Some(place.line), Some(place.columns))
        });
        Self {
            severity,
            source,
            line,
            columns,
            message,
        }
    }

    fn error(source: DiagnosticSource, place: Option<Place>, message: String) -> Self {
        Self::new(Severity::Error, source, place, message)
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// The columns counted in UTF-16 code units, as editors count them, given
    /// the texts that were diagnosed.
    pub fn utf16_columns(&self, text: &str, source_text: Option<&str>) -> Option<Range<usize>> {
        let columns = self.columns.as_ref()?;
        Some(utf16_columns(self.line_text(text, source_text)?, columns))
    }

    /// Renders as `error: line 5, column 12: message`, with the 1-based
    /// column counted in characters of the texts that were diagnosed.
    pub fn display_in<'a>(
        &'a self,
        text: &'a str,
        source_text: Option<&'a str>,
    ) -> impl fmt::Display + 'a {
        let column = self.columns.as_ref().and_then(|columns| {
            let line = self.line_text(text, source_text)?;
            Some(line.get(..columns.start)?.chars().count() + 1)
        });
        DisplayIn {
            diagnostic: self,
            column,
        }
    }

    /// The diagnosed line, from the pipeline text or the separate inventory.
    fn line_text<'a>(&self, text: &'a str, source_text: Option<&'a str>) -> Option<&'a str> {
        let text = match self.source {
            DiagnosticSource::Inventory => source_text?,
            DiagnosticSource::Pipeline => text,
        };
        text.lines().nth(self.line?.checked_sub(1)?)
    }

    fn write(&self, f: &mut fmt::Formatter<'_>, column: Option<usize>) -> fmt::Result {
        write!(f, "{}: ", self.severity.as_str())?;
        let inventory = self.source == DiagnosticSource::Inventory;
        match (self.line, column) {
            (Some(_), _) if inventory => f.write_str("inventory ")?,
            (None, _) if inventory => f.write_str("inventory: ")?,
            _ => {}
        }
        match (self.line, column) {
            (Some(line), Some(column)) => write!(f, "line {line}, column {column}: ")?,
            (Some(line), None) => write!(f, "line {line}: ")?,
            (None, _) => {}
        }
        f.write_str(&self.message)
    }
}

/// A diagnostic rendered with its column; see [`Diagnostic::display_in`].
struct DisplayIn<'a> {
    diagnostic: &'a Diagnostic,
    column: Option<usize>,
}

impl fmt::Display for DisplayIn<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic.write(f, self.column)
    }
}

/// Reads as `error: line 3: message`, naming the inventory for its lines.
/// Without the diagnosed text a column cannot be counted in characters; use
/// [`Diagnostic::display_in`] to include it.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, None)
    }
}

/// Collect independent syntax errors throughout the document, then check its
/// semantics once the document parses: every declaration, step, rule,
/// command, and path error, and warnings for likely mistakes. Jobs are
/// resolved against the inventory only once nothing else is wrong. An absent
/// inventory still allows every other check. Diagnostics are ordered by
/// line, with at most one error per line.
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
    let mut diagnostics: Vec<_> = pipeline_errors
        .into_iter()
        .map(|error| (DiagnosticSource::Pipeline, error))
        .chain(
            inventory_errors
                .into_iter()
                .map(|error| (DiagnosticSource::Inventory, error)),
        )
        .map(|(source, error)| Diagnostic {
            severity: Severity::Error,
            source,
            line: Some(error.line),
            columns: error.columns,
            message: error.message,
        })
        .collect();
    if !diagnostics.is_empty() {
        return finish(diagnostics, text, source_text);
    }

    let document = document.expect("document parsed without errors");
    let inventory_text = source_text.unwrap_or(text);
    diagnostics.extend(pipeline_diagnostics(&document, text, inventory_text));
    if diagnostics.iter().any(Diagnostic::is_error) {
        return finish(diagnostics, text, source_text);
    }
    let inventory = external_inventory
        .or(document.inventory)
        .unwrap_or_default();
    if let Err(error) = resolve(&document.pipeline, &inventory) {
        let (source, place) = error_location(
            &document.pipeline,
            &document.lines,
            &error,
            inventory_text,
            source_text.is_some(),
        );
        diagnostics.push(Diagnostic::error(source, place, error.to_string()));
    }
    finish(diagnostics, text, source_text)
}

/// Flag each `#` that reads like a comment but is part of a word, give every
/// diagnostic with a line its columns, then order.
fn finish(
    mut diagnostics: Vec<Diagnostic>,
    text: &str,
    source_text: Option<&str>,
) -> Vec<Diagnostic> {
    let texts = [
        (DiagnosticSource::Pipeline, Some(text)),
        (DiagnosticSource::Inventory, source_text),
    ];
    for (source, text) in texts {
        let Some(text) = text else { continue };
        for (index, line) in text.lines().enumerate() {
            let Some(word) = glued_comment(line) else {
                continue;
            };
            let number = Some(index + 1);
            let explanation = format!(
                "`#` after `{word}` is part of that word, not a comment; put a space before `#` to start a comment, or quote the text to keep it"
            );
            let errors: Vec<_> = diagnostics
                .iter_mut()
                .filter(|diagnostic| {
                    diagnostic.is_error()
                        && diagnostic.source == source
                        && diagnostic.line == number
                })
                .collect();
            if errors.is_empty() {
                // Point at the word and the `#` joined to it.
                let columns = columns_of(line, word).map(|word| word.start..word.end + 1);
                diagnostics.push(Diagnostic::new(
                    Severity::Warning,
                    source,
                    columns.map(|columns| Place::new(index + 1, columns)),
                    explanation,
                ));
            } else {
                for error in errors {
                    error.message = format!("{} ({explanation})", error.message);
                }
            }
        }
    }
    for diagnostic in &mut diagnostics {
        if diagnostic.columns.is_some() {
            continue;
        }
        let text = match diagnostic.source {
            DiagnosticSource::Inventory => source_text.unwrap_or(text),
            DiagnosticSource::Pipeline => text,
        };
        diagnostic.columns = diagnostic
            .line
            .and_then(|line| text.lines().nth(line.checked_sub(1)?))
            .map(content_columns);
    }
    order(diagnostics)
}

/// Every error in the pipeline text, then warnings about names that did not
/// fail. Needs no inventory.
fn pipeline_diagnostics(
    document: &ParsedDocument,
    text: &str,
    inventory_text: &str,
) -> Vec<Diagnostic> {
    let (pipeline, lines) = (&document.pipeline, &document.lines);
    let checked = collect_pipeline(pipeline);
    let mut diagnostics: Vec<_> = checked
        .errors
        .iter()
        .map(|(subject, error)| {
            let place = subject_place(pipeline, lines, subject, error)
                .or_else(|| error_location(pipeline, lines, error, inventory_text, false).1);
            Diagnostic::error(DiagnosticSource::Pipeline, place, error.to_string())
        })
        .collect();
    let template_errors = collect_commands(pipeline, lines, &checked.poisoned)
        .into_iter()
        .map(|error| (error.line, error.columns, error.focus, error.message))
        .chain(
            collect_paths(pipeline, lines, &checked.poisoned)
                .1
                .into_iter()
                .map(|error| (error.line, error.columns, error.focus, error.message)),
        );
    for (line, columns, focus, message) in template_errors {
        let place = line.zip(columns).map(|(line, columns)| {
            // Narrow to the part the error is about: a placeholder inside the
            // template, or else a name elsewhere on the line, such as the
            // operation a command is declared for.
            let focus = focus.and_then(|focus| {
                let text = text.lines().nth(line.checked_sub(1)?)?;
                let inside = text
                    .get(columns.clone())?
                    .find(&focus)
                    .map(|offset| columns.start + offset..columns.start + offset + focus.len());
                inside.or_else(|| find_word(text, content_columns(text).start, &focus))
            });
            Place::new(line, focus.unwrap_or(columns))
        });
        diagnostics.push(Diagnostic::error(
            DiagnosticSource::Pipeline,
            place,
            message,
        ));
    }
    diagnostics.extend(warnings(pipeline, lines, &checked.poisoned));
    diagnostics
}

/// The part of a declaration, step, or rule that a pipeline error is about.
fn subject_place(
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
        DefinitionSubject::Source(_) | DefinitionSubject::None => None,
    }
}

/// The part of the step producing `output` that `error` is about: an input,
/// the output, the operation name, or the whole call.
fn step_part(pipeline: &Pipeline, step: &Step, output: &str, error: &ResolveError) -> Place {
    let invocation = pipeline
        .invocations
        .iter()
        .find(|invocation| invocation.output_product == output);
    let input_named = |product: &str| {
        let index = invocation?
            .inputs
            .iter()
            .position(|binding| binding.product_name() == product)?;
        step.input(index)
    };
    let port = |port: &str| {
        if port == "output" {
            return Some(step.output());
        }
        let invocation = invocation?;
        let operation = pipeline
            .operations
            .iter()
            .find(|operation| operation.name == invocation.operation)?;
        let index = operation
            .inputs
            .iter()
            .position(|input| input.name == port)?;
        step.input(index)
    };
    let part = match error {
        ResolveError::UnknownProduct { name } if name == output => Some(step.output()),
        ResolveError::UnknownProduct { name } => input_named(name),
        ResolveError::UnknownOperation { .. } => Some(step.operation()),
        ResolveError::TypeMismatch { port: name, .. }
        | ResolveError::TypeVariableConflict { port: name, .. }
        | ResolveError::MissingInput { port: name, .. } => port(name),
        ResolveError::InvalidAggregationDimension { product, .. } => input_named(product),
        ResolveError::UnsupportedShapeRelationship { .. } => Some(step.call()),
        _ => Some(step.output()),
    };
    part.unwrap_or_else(|| step.call())
}

/// Order by source and line, keep only the first error on each line, and
/// drop warnings on a line that already has an error.
fn order(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let key = |diagnostic: &Diagnostic| {
        (
            diagnostic.source == DiagnosticSource::Inventory,
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
fn warnings(pipeline: &Pipeline, lines: &SourceMap, skip: &BTreeSet<String>) -> Vec<Diagnostic> {
    let warn = |place: Option<Place>, message| {
        Diagnostic::new(
            Severity::Warning,
            DiagnosticSource::Pipeline,
            place,
            message,
        )
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
                lines.products.get(name).cloned(),
                format!("source product `{name}` is never used as an input"),
            ));
        }
    }
    for operation in &pipeline.operations {
        let name = operation.name.as_str();
        if skip.contains(name) {
            continue;
        }
        let place = lines.operations.get(name).cloned();
        let imported = lines.imported.contains(name);
        if !used_operations.contains(name) {
            if !imported && !library {
                warnings.push(warn(
                    place.clone(),
                    format!("operation `{name}` is declared but never used"),
                ));
            }
        } else if !pipeline.commands.is_empty() && !commands.contains(name) {
            // Only once commands are in use: a pipeline may be written for its DAG alone.
            warnings.push(warn(
                place.clone(),
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
                place.clone(),
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
) -> (DiagnosticSource, Option<Place>) {
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
            .filter_map(|invocation| step(&invocation.output_product));
        let first = matching.next()?;
        matching.next().is_none().then_some(first)
    };
    let pipeline_place = match error {
        ResolveError::TypeMismatch { output_product, .. }
        | ResolveError::TypeVariableConflict { output_product, .. }
        | ResolveError::MissingInput { output_product, .. } => step(output_product),
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
            .and_then(|invocation| step(&invocation.output_product))
            .or_else(|| lines.constraints.get(name).map(Rule::product)),
        ResolveError::InvalidAggregationDimension { product, dimension } => pipeline
            .invocations
            .iter()
            .find(|invocation| {
                invocation.inputs.iter().any(|binding| {
                    matches!(binding, InputBinding::Vary { product: name, dimension: axis }
                        if name == product && axis == dimension)
                })
            })
            .and_then(|invocation| step(&invocation.output_product)),
        ResolveError::Cycle { products } => products.first().and_then(|name| step(name)),
        ResolveError::UnsupportedShapeRelationship { operation, .. } => unique_step(operation),
        // Too few or too many artifacts is about the rule as a whole.
        ResolveError::CoverageViolation { rule_index, .. } => {
            lines.rules.get(*rule_index).map(Rule::whole)
        }
        ResolveError::DuplicateOutputArtifact { artifact } => step(&artifact.product),
        ResolveError::InvalidDefinition { subject, .. } => {
            subject_place(pipeline, lines, subject, error)
        }
        ResolveError::DuplicateSourceArtifact { .. } => None,
    };
    if pipeline_place.is_some() {
        return (DiagnosticSource::Pipeline, pipeline_place);
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
    // A source record is one line; point at all of it.
    let inventory_place = inventory_line.and_then(|line| {
        let text = inventory_text.lines().nth(line.checked_sub(1)?)?;
        Some(Place::new(line, content_columns(text)))
    });
    match inventory_place {
        Some(_) if external_inventory => (DiagnosticSource::Inventory, inventory_place),
        _ => (DiagnosticSource::Pipeline, inventory_place),
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
