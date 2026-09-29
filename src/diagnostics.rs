//! Editor-friendly validation of an in-memory SPIT document.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::path::Path;

use crate::command::collect_commands;
use crate::compile::collect_pipeline;
use crate::imports::parse_located_document;
use crate::inputs::{check_inventory, collect_rule_errors, InputSpec};
use crate::json::Json;
use crate::lower::{parse_document_with_imports, ParsedDocument};
use crate::model::DEFAULT_OUTPUT;
use crate::model::{stage_within, CommandRole, Job, ResolvedDag, SourceInventory};
use crate::parser::{glued_comment, source_record_lines, Kind, Rule, SourceMap, Step};
use crate::paths::{case_collisions, collect_paths};
use crate::span::{content_columns, utf16_columns, Located, Place};
use crate::{
    parse_source_inventory, resolve, resolve_artifacts_excluding, DefinitionSubject, EntityBinding,
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

/// Validation results with the successfully parsed values retained for a
/// caller that will execute the checked pipeline.
pub struct Diagnosis {
    pub diagnostics: Vec<Diagnostic>,
    pub pipeline: Option<Pipeline>,
    pub inventory: Option<SourceInventory>,
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

    /// An error that records its own location, narrowed to what it is about
    /// within `text`, the text it is in.
    fn located<E: fmt::Display>(source: DiagnosticSource, error: &Located<E>, text: &str) -> Self {
        let place = error.location.place_in(text);
        Self {
            severity: Severity::Error,
            source,
            line: error.location.line,
            columns: place.map(|place| place.columns),
            message: error.message(),
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

/// The diagnostics as the JSON `check --json` prints, for editors. Columns
/// are 1-based, in UTF-16 code units as editors count them; `end_column` is
/// one past the last character.
pub fn render_diagnostics_json(
    diagnostics: &[Diagnostic],
    text: &str,
    source_text: Option<&str>,
) -> String {
    let items = diagnostics.iter().map(|diagnostic| {
        let columns = diagnostic.utf16_columns(text, source_text);
        Json::object([
            ("severity", Json::string(diagnostic.severity.as_str())),
            ("source", Json::string(diagnostic.source.as_str())),
            ("line", Json::number_or_null(diagnostic.line)),
            (
                "column",
                Json::number_or_null(columns.as_ref().map(|columns| columns.start + 1)),
            ),
            (
                "end_column",
                Json::number_or_null(columns.as_ref().map(|columns| columns.end + 1)),
            ),
            ("message", Json::string(&diagnostic.message)),
        ])
    });
    format!("{}\n", Json::object([("diagnostics", Json::array(items))]))
}

/// Collect independent syntax errors throughout the document, then check its
/// semantics once the document parses: every declaration, step, rule,
/// command, and path error, and warnings for likely mistakes. Jobs are
/// resolved against the inventory only once nothing else is wrong. An absent
/// inventory still allows every other check. Diagnostics are ordered by
/// line, with at most one error per line.
pub fn diagnose(text: &str, source_text: Option<&str>) -> Vec<Diagnostic> {
    diagnose_with_parser(text, source_text, false, |text| {
        parse_document_with_imports(text, &BTreeMap::new(), Kind::Pipeline)
    })
    .diagnostics
}

/// Diagnose a document with its location available for resolving imports.
pub fn diagnose_at(text: &str, source_text: Option<&str>, path: &Path) -> Vec<Diagnostic> {
    diagnose_at_checked(text, source_text, path, None, false).diagnostics
}

pub fn diagnose_at_checked(
    text: &str,
    source_text: Option<&str>,
    path: &Path,
    inputs: Option<&InputSpec>,
    lenient: bool,
) -> Diagnosis {
    let original = RefCell::new(None);
    let mut diagnosis = diagnose_with_parser(text, source_text, lenient, |text| {
        let mut document = parse_located_document(text, path, Kind::Pipeline)?;
        if let Some(inputs) = inputs {
            inputs
                .check(&document.pipeline)
                .map_err(|error| ParseError::new(1, error.to_string()))?;
            *original.borrow_mut() = Some(document.pipeline.clone());
            inputs.apply_paths(&mut document.pipeline);
            document.inputs = inputs.rules.clone();
        }
        Ok(document)
    });
    if diagnosis.pipeline.is_some() && inputs.is_some() {
        diagnosis.pipeline = original.into_inner();
    }
    diagnosis
}

/// Diagnose a pipeline after attaching a separately parsed `.spitin` recipe.
pub fn diagnose_at_with_inputs(
    text: &str,
    source_text: Option<&str>,
    path: &Path,
    inputs: &InputSpec,
    lenient: bool,
) -> Vec<Diagnostic> {
    diagnose_at_checked(text, source_text, path, Some(inputs), lenient).diagnostics
}

/// Diagnose a `.spitin` recipe at `path` without reading any data: its own
/// lines, then the pipeline its `pipeline` line names, then its rules and
/// any records against that pipeline. The pipeline's own errors are named
/// by file and line, since they are not in `text`.
pub fn diagnose_recipe(text: &str, path: &Path) -> Vec<Diagnostic> {
    let error = |message: String| Diagnostic::error(DiagnosticSource::Pipeline, None, message);
    let spec = match crate::inputs::parse_input_spec_at(text, path) {
        Ok(spec) => spec,
        Err(parse) => {
            let diagnostic = Diagnostic::located(DiagnosticSource::Pipeline, &parse, text);
            return finish(vec![diagnostic], text, None);
        }
    };
    let Some(pipeline_path) = spec.pipeline.clone() else {
        let message =
            "name the pipeline this recipe is for, with a line such as `pipeline analysis.spit`";
        return finish(vec![error(message.to_owned())], text, None);
    };
    let shown = pipeline_path.display();
    let pipeline_text = match std::fs::read_to_string(&pipeline_path) {
        Ok(pipeline_text) => pipeline_text,
        Err(reason) => {
            return finish(
                vec![error(format!("cannot read pipeline `{shown}`: {reason}"))],
                text,
                None,
            )
        }
    };
    let checked = diagnose_at_checked(&pipeline_text, None, &pipeline_path, None, false);
    let pipeline_errors: Vec<_> = checked
        .diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .map(|diagnostic| {
            let line = diagnostic
                .line
                .map_or_else(String::new, |line| format!(" line {line}"));
            error(format!("in `{shown}`{line}: {}", diagnostic.message))
        })
        .collect();
    if !pipeline_errors.is_empty() {
        return finish(pipeline_errors, text, None);
    }
    diagnose_recipe_against(text, &checked.pipeline.expect("pipeline passed diagnosis"))
}

/// Diagnose the text of a `.spitin` recipe against `pipeline`, reading no
/// data: every rule's error at the rule, then its source paths and records.
pub fn diagnose_recipe_against(text: &str, pipeline: &Pipeline) -> Vec<Diagnostic> {
    let (spec, lines) = match crate::inputs::parse_recipe_lines(text) {
        Ok(parsed) => parsed,
        Err(parse) => {
            let diagnostic = Diagnostic::located(DiagnosticSource::Pipeline, &parse, text);
            return finish(vec![diagnostic], text, None);
        }
    };
    let mut diagnostics: Vec<_> = collect_rule_errors(pipeline, &spec.rules, &BTreeSet::new())
        .into_iter()
        .map(|(subject, error)| {
            // A rule's own errors concern its product, or its groups.
            let place = match &subject {
                DefinitionSubject::Constraint(index) => lines.rules.get(*index).map(Rule::product),
                DefinitionSubject::ConstraintGroup(index) => {
                    lines.rules.get(*index).map(Rule::dimensions)
                }
                _ => None,
            };
            Diagnostic::error(DiagnosticSource::Pipeline, place, error.to_string())
        })
        .collect();
    if diagnostics.is_empty() {
        let checked = match &spec.inventory {
            Some(records) => spec
                .resolve(pipeline, crate::InputSource::Inventory(records.clone()))
                .map(|_| ()),
            None => spec.check(pipeline),
        };
        if let Err(problem) = checked {
            // Records written in the recipe keep its line numbers.
            let place = problem
                .downcast_ref::<ResolveError>()
                .and_then(|error| error_location(pipeline, &lines, error, text, false).1);
            diagnostics.push(Diagnostic::error(
                DiagnosticSource::Pipeline,
                place,
                problem.to_string(),
            ));
        }
    }
    finish(diagnostics, text, None)
}

pub fn diagnose_artifacts_at(
    text: &str,
    source_text: Option<&str>,
    path: &Path,
) -> Vec<Diagnostic> {
    diagnose_at_checked(text, source_text, path, None, true).diagnostics
}

fn diagnose_with_parser(
    text: &str,
    source_text: Option<&str>,
    lenient: bool,
    parser: impl Fn(&str) -> Result<ParsedDocument, ParseError>,
) -> Diagnosis {
    let (document, pipeline_errors) = recover_parse_errors(text, parser);
    let (external_inventory, inventory_errors) = source_text.map_or_else(
        || (None, Vec::new()),
        |source_text| recover_parse_errors(source_text, parse_source_inventory),
    );
    let mut diagnostics: Vec<_> = pipeline_errors
        .iter()
        .map(|error| Diagnostic::located(DiagnosticSource::Pipeline, error, text))
        .chain(inventory_errors.iter().map(|error| {
            Diagnostic::located(
                DiagnosticSource::Inventory,
                error,
                source_text.unwrap_or(text),
            )
        }))
        .collect();
    if !diagnostics.is_empty() {
        return Diagnosis {
            diagnostics: finish(diagnostics, text, source_text),
            pipeline: None,
            inventory: None,
        };
    }

    let document = document.expect("document parsed without errors");
    let inventory_text = source_text.unwrap_or(text);
    diagnostics.extend(pipeline_diagnostics(&document, text, inventory_text));
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Diagnosis {
            diagnostics: finish(diagnostics, text, source_text),
            pipeline: None,
            inventory: None,
        };
    }
    // Without records the input stage and jobs have nothing to work on.
    let Some(supplied) = external_inventory else {
        return Diagnosis {
            diagnostics: finish(diagnostics, text, source_text),
            pipeline: Some(document.pipeline),
            inventory: None,
        };
    };
    let inventory = supplied.clone();
    let outputs = |jobs: Vec<Job>| jobs.into_iter().flat_map(|job| job.outputs);
    // The input stage settles the inventory; only then are jobs resolved.
    let produced: Result<BTreeSet<String>, _> =
        check_inventory(&document.pipeline, &document.inputs, &inventory).and_then(|checked| {
            let unavailable: Vec<_> = checked
                .gaps
                .iter()
                .flat_map(|gap| gap.sources.iter().cloned())
                .collect();
            if lenient {
                let report = resolve_artifacts_excluding(
                    &document.pipeline,
                    &checked.inventory,
                    &unavailable,
                )?;
                let incomplete = report.incomplete.into_iter().flat_map(|job| job.outputs);
                Ok(outputs(report.dag.jobs)
                    .chain(incomplete)
                    .map(|artifact| artifact.product)
                    .collect())
            } else {
                if let Some(gap) = checked.gaps.into_iter().next() {
                    return Err(gap.error);
                }
                let dag = resolve(&document.pipeline, &checked.inventory)?;
                diagnostics.extend(case_warnings(&document.pipeline, &document.lines, &dag));
                Ok(outputs(dag.jobs).map(|artifact| artifact.product).collect())
            }
        });
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
            diagnostics.extend(empty_step_warnings(
                &document.pipeline,
                &document.lines,
                &produced,
                &supplied,
            ));
        }
    }
    Diagnosis {
        diagnostics: finish(diagnostics, text, source_text),
        pipeline: Some(document.pipeline),
        inventory: Some(supplied),
    }
}

/// Flag paths that differ only in case, which are one file on macOS and Windows.
fn case_warnings(pipeline: &Pipeline, lines: &SourceMap, dag: &ResolvedDag) -> Vec<Diagnostic> {
    case_collisions(pipeline, dag)
        .into_iter()
        .map(|[(first, first_path), (second, second_path)]| {
            warning(
                lines.path_rule(pipeline, &second.0),
                format!(
                    "`{}[{}]` and `{}[{}]` have paths `{first_path}` and `{second_path}`, which differ only in case, so they are one file where case is ignored, as on macOS and Windows",
                    first.0, first.1, second.0, second.1
                ),
            )
        })
        .collect()
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
            let Some(columns) = glued_comment(line) else {
                continue;
            };
            let word = &line[columns.clone()];
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
                diagnostics.push(Diagnostic::new(
                    Severity::Warning,
                    source,
                    Some(Place::new(index + 1, columns.start..columns.end + 1)),
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
    let rule_errors = collect_rule_errors(pipeline, &document.inputs, &checked.poisoned);
    let mut diagnostics: Vec<_> = checked
        .errors
        .iter()
        .chain(&rule_errors)
        .map(|(subject, error)| {
            let place = subject_place(pipeline, lines, subject, error)
                .or_else(|| error_location(pipeline, lines, error, inventory_text, false).1);
            Diagnostic::error(DiagnosticSource::Pipeline, place, error.to_string())
        })
        .collect();
    let template_errors = collect_commands(pipeline, lines, &checked.poisoned)
        .into_iter()
        .chain(collect_paths(pipeline, lines, &checked.poisoned).1);
    diagnostics.extend(
        template_errors.map(|error| Diagnostic::located(DiagnosticSource::Pipeline, &error, text)),
    );
    diagnostics.extend(warnings(pipeline, lines, &checked.poisoned));
    diagnostics.extend(operator_warnings(pipeline, lines, text));
    diagnostics
}

/// Flag unquoted shell operators such as `>` or `|`, which SPIT passes to
/// the program as arguments.
fn operator_warnings(pipeline: &Pipeline, lines: &SourceMap, text: &str) -> Vec<Diagnostic> {
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
                    Place::new(place.line, columns.unwrap_or(place.columns.clone()))
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
    // A library, with no steps, may declare what it never uses.
    if pipeline.invocations.is_empty() {
        return operation_warnings(pipeline, lines, skip, true);
    }
    let mut warnings = stage_warnings(pipeline, lines);
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
    let original_lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut recovered = original_lines.join("\n");
    let mut offset = 0;
    let ranges: Vec<_> = original_lines
        .iter()
        .map(|line| {
            let range = offset..offset + line.len();
            offset = range.end + 1;
            range
        })
        .collect();
    let mut errors = Vec::new();
    loop {
        match parse(&recovered) {
            Ok(parsed) => return (Some(parsed), errors),
            Err(error) => {
                let Some(range) = error
                    .line()
                    .checked_sub(1)
                    .and_then(|index| ranges.get(index))
                    .filter(|range| !recovered[(*range).clone()].trim().is_empty())
                else {
                    errors.push(error);
                    return (None, errors);
                };
                recovered.replace_range(range.clone(), &" ".repeat(range.len()));
                // Misplaced records are one error, however many lines.
                if let ParseErrorKind::MisplacedRecords { lines: records } = &error.kind {
                    for record in records {
                        if let Some(range) = ranges.get(record - 1) {
                            recovered.replace_range(range.clone(), &" ".repeat(range.len()));
                        }
                    }
                }
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
            .get(previous.line().saturating_sub(1))
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
    source_record_lines(text)
        .into_iter()
        .filter_map(|(line, record)| {
            (record.product == product && entities.is_none_or(|value| value == &record.entities))
                .then_some(line)
        })
        .collect()
}
