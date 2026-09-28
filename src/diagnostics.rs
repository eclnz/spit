//! Editor-friendly validation of an in-memory SPIT document.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::path::Path;

use crate::command::collect_commands;
use crate::imports::parse_located_document;
use crate::model::DEFAULT_OUTPUT;
use crate::model::{stage_within, CommandRole, Job, SourceInventory};
use crate::parser::{
    glued_comment, parse_document_with_imports, InlineInventory, ParsedDocument, Rule, SourceMap,
    Step,
};
use crate::paths::collect_paths;
use crate::resolver::collect_pipeline;
use crate::span::{columns_of, content_columns, find_word, utf16_columns, Place};
use crate::{
    parse_source_inventory, resolve, resolve_artifacts, DefinitionSubject, EntityBinding,
    InputBinding, ParseError, ParseErrorKind, Pipeline, ResolveError,
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
    let inline = inline_inventory(source_text);
    diagnose_with_parser(text, source_text, false, |text| {
        parse_document_with_imports(text, &BTreeMap::new(), inline)
    })
}

/// Diagnose a document with its location available for resolving imports.
pub fn diagnose_at(text: &str, source_text: Option<&str>, path: &Path) -> Vec<Diagnostic> {
    let inline = inline_inventory(source_text);
    diagnose_with_parser(text, source_text, false, |text| {
        parse_located_document(text, path, inline)
    })
}

pub fn diagnose_artifacts_at(
    text: &str,
    source_text: Option<&str>,
    path: &Path,
) -> Vec<Diagnostic> {
    let inline = inline_inventory(source_text);
    diagnose_with_parser(text, source_text, true, |text| {
        parse_located_document(text, path, inline)
    })
}

/// A separate inventory replaces an inline one, which is then not read.
fn inline_inventory(source_text: Option<&str>) -> InlineInventory {
    if source_text.is_some() {
        InlineInventory::Skip
    } else {
        InlineInventory::Read
    }
}

fn diagnose_with_parser(
    text: &str,
    source_text: Option<&str>,
    lenient: bool,
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
    if let (Some(line), Some(_)) = (document.inventory_line, source_text) {
        let place = text
            .lines()
            .nth(line - 1)
            .map(|header| Place::new(line, content_columns(header)));
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            DiagnosticSource::Pipeline,
            place,
            "this inline inventory is ignored because a separate inventory was supplied".to_owned(),
        ));
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        return finish(diagnostics, text, source_text);
    }
    let supplied = external_inventory.or(document.inventory);
    let inventory = supplied.clone().unwrap_or_default();
    let outputs = |jobs: Vec<Job>| jobs.into_iter().flat_map(|job| job.outputs);
    let produced: Result<BTreeSet<String>, _> = if lenient {
        resolve_artifacts(&document.pipeline, &inventory).map(|report| {
            let incomplete = report.incomplete.into_iter().flat_map(|job| job.outputs);
            outputs(report.dag.jobs)
                .chain(incomplete)
                .map(|artifact| artifact.product)
                .collect()
        })
    } else {
        resolve(&document.pipeline, &inventory)
            .map(|dag| outputs(dag.jobs).map(|artifact| artifact.product).collect())
    };
    match produced {
        Err(error) => {
            let (source, place) = error_location(
                &document.pipeline,
                &document.lines,
                &error,
                inventory_text,
                source_text.is_some(),
            );
            diagnostics.push(Diagnostic::error(source, place, error.to_string()));
        }
        // Without an inventory no step is expected to resolve jobs.
        Ok(produced) => {
            if let Some(inventory) = &supplied {
                diagnostics.extend(empty_step_warnings(
                    &document.pipeline,
                    &document.lines,
                    &produced,
                    inventory,
                ));
            }
        }
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
        ResolveError::UnknownProduct { name } if name == output => Some(step.output()),
        ResolveError::UnknownProduct { name } => input_named(name),
        ResolveError::UnknownOperation { .. } => Some(step.operation()),
        ResolveError::TypeMismatch { port: name, .. }
        | ResolveError::TypeVariableConflict { port: name, .. }
        | ResolveError::MissingInput { port: name, .. }
        | ResolveError::AmbiguousInput { port: name, .. }
        | ResolveError::CollectionTooSmall { port: name, .. } => port(name),
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
        .flat_map(|invocation| invocation.outputs.iter().map(String::as_str))
        .collect();
    let inputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| invocation.inputs.iter().map(InputBinding::product_name))
        .collect();
    let commands: BTreeSet<_> = pipeline
        .commands
        .iter()
        .filter(|command| command.role == CommandRole::Run)
        .map(|command| command.operation.as_str())
        .collect();

    let library = pipeline.invocations.is_empty();
    let mut warnings = Vec::new();
    // A library may group its operations in stages that hold no steps.
    for stage in pipeline.stages.iter().filter(|_| !library) {
        let name = stage.name.as_str();
        // A stage whose steps all sit in stages nested in it is not empty.
        if !pipeline.invocations.iter().any(|invocation| {
            invocation
                .stage
                .as_deref()
                .is_some_and(|stage| stage_within(stage, name))
        }) {
            warnings.push(warn(
                lines.stages.get(name).cloned(),
                format!("stage `{name}` has no steps"),
            ));
        }
    }
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
        let produced: BTreeSet<_> = operation
            .outputs
            .iter()
            .flat_map(|port| port.artifact_type.variables())
            .collect();
        for variable in produced.difference(&bound) {
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

/// Steps that resolve no jobs from a supplied inventory. A source with no
/// artifacts is reported once, naming the steps it leaves empty; any other
/// step that is empty although its inputs are not is reported on its own.
fn empty_step_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    produced: &BTreeSet<String>,
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
    // The unobserved sources each product depends on.
    fn unobserved<'a>(
        name: &'a str,
        producers: &BTreeMap<&'a str, &'a crate::Invocation>,
        observed: &BTreeSet<&str>,
        found: &mut BTreeSet<&'a str>,
    ) {
        match producers.get(name) {
            Some(invocation) => {
                for input in &invocation.inputs {
                    unobserved(&input.product, producers, observed, found);
                }
            }
            None if !observed.contains(name) => {
                found.insert(name);
            }
            None => {}
        }
    }
    let mut left_empty: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut warnings = Vec::new();
    for invocation in &pipeline.invocations {
        let step = invocation.output_product();
        if produced.contains(step) {
            continue;
        }
        let mut sources = BTreeSet::new();
        for input in &invocation.inputs {
            unobserved(&input.product, &producers, &observed, &mut sources);
        }
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
            .filter_map(|invocation| step(invocation.output_product()));
        let first = matching.next()?;
        matching.next().is_none().then_some(first)
    };
    let pipeline_place = match error {
        ResolveError::TypeMismatch { output_product, .. }
        | ResolveError::TypeVariableConflict { output_product, .. }
        | ResolveError::MissingInput { output_product, .. }
        | ResolveError::AmbiguousInput { output_product, .. }
        | ResolveError::CollectionTooSmall { output_product, .. } => step(output_product),
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
                invocation.inputs.iter().any(|binding| {
                    &binding.product == product && binding.vary.as_ref() == Some(dimension)
                })
            })
            .and_then(|invocation| step(invocation.output_product())),
        ResolveError::Cycle { products } => products.first().and_then(|name| step(name)),
        ResolveError::UnsupportedShapeRelationship { operation, .. } => unique_step(operation),
        // Too few or too many artifacts is about the rule as a whole.
        ResolveError::CoverageViolation { rule_index, .. }
        | ResolveError::MissingRequiredValue { rule_index, .. } => {
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
