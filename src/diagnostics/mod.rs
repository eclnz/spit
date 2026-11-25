mod diagnostic;
mod places;
mod recipe;
mod recovery;

pub use recipe::{diagnose_recipe, diagnose_recipe_against};
mod warnings;

pub use diagnostic::*;
pub(crate) use recovery::recover_document;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::inputs::{check_inventory, with_source_paths, InputError, InputSpec, ResolvedInputs};
use crate::lower::imports::parse_located_document_recovering;
use crate::lower::{parse_document_recovering, ParsedDocument};
use crate::model::{ArtifactReport, CoverageGap, InputRules, PipelineIndex, SourceInventory};
use crate::parser::{as_read_back, parse_inventory_recovering, without_bom, Kind, Rule, SourceMap};
use crate::resolver::first_failure;
use crate::span::{content_columns, glued_comment, Lines, Place};
use crate::template::PathTemplate;
use crate::validate::check_pipeline;
use crate::{
    parse_source_inventory, resolve_potential_artifacts, DefinitionSubject, ParseError,
    ParseErrorKind, Pipeline, ResolveError,
};

use places::{error_step, InCall, Places};
use recovery::recover_parse_errors;
use warnings::{empty_step_warnings, near_miss_warnings, path_warnings, pipeline_warning};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Completeness {
    #[default]
    Required,
    Reported,
    Partial,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    pub path: Option<&'a Path>,
    pub recipe: Option<&'a InputSpec>,
    pub completeness: Completeness,
}

impl<'a> Context<'a> {
    pub fn at(path: &'a Path) -> Self {
        Self {
            path: Some(path),
            ..Self::default()
        }
    }

    fn parse(&self, text: &str) -> (Parsed, Vec<ParseError>) {
        let (mut document, mut errors) = match self.path {
            Some(path) => parse_located_document_recovering(text, path, Kind::Pipeline),
            None => parse_document_recovering(text, &BTreeMap::new(), Kind::Pipeline),
        };
        let Some(recipe) = self.recipe else {
            let parsed = Parsed {
                document,
                as_written: None,
            };
            return (parsed, errors);
        };
        if let Err(error) = recipe.check(&document.pipeline) {
            errors.push(ParseError::of(
                1,
                ParseErrorKind::Other {
                    message: error.to_string(),
                },
            ));
        }
        let as_written = document.pipeline.clone();
        recipe.apply_paths(&mut document.pipeline);
        document.inputs = recipe.rules.clone();
        let parsed = Parsed {
            document,
            as_written: Some(as_written),
        };
        (parsed, errors)
    }
}

struct Parsed {
    document: ParsedDocument,
    as_written: Option<Pipeline>,
}

impl Parsed {
    fn checked(self, warnings: Vec<Diagnostic>) -> Checked {
        let paths = shown_paths(&self.document.pipeline, &self.document.lines);
        Checked {
            pipeline: self.as_written.unwrap_or(self.document.pipeline),
            warnings,
            paths,
        }
    }
}

fn shown_paths(pipeline: &Pipeline, lines: &SourceMap) -> Vec<ShownPath> {
    let index = PipelineIndex::new(pipeline);
    let templates = index.path_templates();
    let mut shown: Vec<_> = pipeline
        .products
        .iter()
        .zip(&templates)
        .filter(|(product, _)| !lines.imported.contains(&product.name))
        .filter(|(product, _)| {
            !pipeline.product_paths.contains_key(&product.name)
                || index.added_extension(&product.name).is_some()
                || index
                    .path_rule_for(&product.name)
                    .is_some_and(PathTemplate::varies)
        })
        .filter_map(|(product, template)| {
            let line = match lines.invocations.get(&product.name) {
                Some(step) => step.line,
                None => lines.products.get(&product.name)?.line,
            };
            let paths = crate::paths::shown_paths(&index, &product.name, template.as_deref()?);
            Some(paths.into_iter().map(move |path| ShownPath {
                product: product.name.clone(),
                line,
                path,
            }))
        })
        .flatten()
        .collect();
    shown.sort_by_key(|path| path.line);
    shown
}

pub fn diagnose(text: &str, source_text: Option<&str>) -> Vec<Diagnostic> {
    diagnose_in(text, source_text, Context::default())
}

pub fn diagnose_in(text: &str, source_text: Option<&str>, context: Context<'_>) -> Vec<Diagnostic> {
    let diagnosis = match source_text {
        None => diagnose_checked(text, context),
        Some(records) => {
            diagnose_checked_with_records(text, records, context).map(|(checked, _)| checked)
        }
    };
    diagnosis.map_or_else(|all| all, |checked| checked.warnings)
}

pub fn diagnose_checked(text: &str, context: Context<'_>) -> Diagnosis {
    let text = without_bom(text);
    let parsed = recover_parse_errors(text, |text| context.parse(text)).map_err(|errors| {
        let diagnostics = located_all(DiagnosticSource::Pipeline, errors, text);
        finish(diagnostics.collect(), text, None)
    })?;
    let warnings = check_document(&parsed.document, text, None)?;
    Ok(parsed.checked(finish(warnings, text, None)))
}

pub fn diagnose_checked_with_records(
    text: &str,
    records: &str,
    context: Context<'_>,
) -> Diagnosis<(Checked, Records)> {
    let (text, records) = (without_bom(text), without_bom(records));
    let parsed = (
        recover_parse_errors(text, |text| context.parse(text)),
        recover_parse_errors(records, parse_inventory_recovering),
    );
    match parsed {
        (Ok(parsed), Ok(inventory)) => {
            diagnose_over(parsed, inventory, None, text, Some(records), context)
        }
        (parsed, inventory) => {
            let pipeline_errors = parsed.err().into_iter().flatten();
            let record_errors = inventory.err().into_iter().flatten();
            let diagnostics = located_all(DiagnosticSource::Pipeline, pipeline_errors, text)
                .chain(located_all(
                    DiagnosticSource::Inventory,
                    record_errors,
                    records,
                ))
                .collect();
            Err(finish(diagnostics, text, Some(records)))
        }
    }
}

pub fn diagnose_checked_with_inventory(
    text: &str,
    settled: &ResolvedInputs,
    context: Context<'_>,
) -> Option<(Checked, Records)> {
    let text = without_bom(text);
    let parsed = recover_parse_errors(text, |text| context.parse(text)).ok()?;
    let no_rules = InputRules::default();
    let rules = context.recipe.map_or(&no_rules, |recipe| &recipe.rules);
    let inventory = as_read_back(&settled.inventory, &parsed.document.pipeline, rules)?;
    let gaps = Some(settled.gaps.as_slice());
    let (checked, records) = diagnose_over(parsed, inventory, gaps, text, None, context).ok()?;
    let in_records = |diagnostic: &Diagnostic| diagnostic.source == DiagnosticSource::Inventory;
    if checked.warnings.iter().any(in_records) {
        return None;
    }
    Some((checked, records))
}

fn diagnose_over(
    parsed: Parsed,
    inventory: SourceInventory,
    gaps: Option<&[CoverageGap]>,
    text: &str,
    records: Option<&str>,
    context: Context<'_>,
) -> Diagnosis<(Checked, Records)> {
    let document = &parsed.document;
    let mut diagnostics = check_document(document, text, records)?;
    let records_text = records.unwrap_or_default();
    let report = record_diagnostics(
        document,
        &inventory,
        gaps,
        records_text,
        context.completeness,
    );
    let report = match report {
        Ok((report, found)) => {
            diagnostics.extend(found);
            Some(report)
        }
        Err(found) => {
            diagnostics.extend(found);
            None
        }
    };
    let diagnostics = finish(diagnostics, text, records);
    match report {
        Some(report) if !diagnostics.iter().any(Diagnostic::is_error) => {
            Ok((parsed.checked(diagnostics), Records { inventory, report }))
        }
        _ => Err(diagnostics),
    }
}

pub fn diagnose_inputs(text: &str) -> Vec<Diagnostic> {
    let text = without_bom(text);
    match parse_source_inventory(text) {
        Ok(_) => Vec::new(),
        Err(parse) => {
            let diagnostic =
                Diagnostic::located("parse", DiagnosticSource::Inventory, &parse, text);
            finish(vec![diagnostic], text, Some(text))
        }
    }
}

pub fn diagnose_tools(text: &str) -> Vec<Diagnostic> {
    let text = without_bom(text);
    match crate::inputs::parse_tools(text) {
        Ok(_) => Vec::new(),
        Err(parse) => {
            let diagnostic = Diagnostic::located("parse", DiagnosticSource::Pipeline, &parse, text);
            finish(vec![diagnostic], text, None)
        }
    }
}

fn located_all<'a>(
    source: DiagnosticSource,
    errors: impl IntoIterator<Item = ParseError> + 'a,
    text: &'a str,
) -> impl Iterator<Item = Diagnostic> + 'a {
    errors
        .into_iter()
        .map(move |error| Diagnostic::located("parse", source, &error, text))
}

fn check_document(
    document: &ParsedDocument,
    text: &str,
    source_text: Option<&str>,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let diagnostics = pipeline_diagnostics(document, text, source_text.unwrap_or(text));
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(finish(diagnostics, text, source_text));
    }
    Ok(diagnostics)
}

fn record_diagnostics(
    document: &ParsedDocument,
    supplied: &SourceInventory,
    settled: Option<&[CoverageGap]>,
    records: &str,
    completeness: Completeness,
) -> Result<(ArtifactReport, Vec<Diagnostic>), Vec<Diagnostic>> {
    let (pipeline, lines) = (&document.pipeline, &document.lines);
    let mut diagnostics = Vec::new();
    let checked = match settled {
        Some(gaps) => Ok(Cow::Borrowed(gaps)),
        None => check_inventory(pipeline, &document.inputs, supplied).map(Cow::Owned),
    };
    let resolved = checked.and_then(|gaps| {
        if completeness != Completeness::Required {
            let unavailable: Vec<_> = gaps
                .iter()
                .flat_map(|gap| gap.sources.iter().cloned())
                .collect();
            let partial = completeness == Completeness::Partial;
            let report = resolve_potential_artifacts(pipeline, supplied, &unavailable, partial)?;
            diagnostics.extend(near_miss_warnings(&report));
            return Ok(report);
        }
        if let Some(gap) = gaps.first() {
            return Err(gap.error.clone());
        }
        let report = resolve_potential_artifacts(pipeline, supplied, &[], false)?;
        diagnostics.extend(near_miss_warnings(&report));
        if let Some(error) = first_failure(&report.incomplete) {
            let remaining: usize = report.incomplete.iter().map(|job| job.outputs.len()).sum();
            let places = Places::new(pipeline, lines);
            let (source, place) = places.error_location(&error, records, true);
            let mut message = error.to_string();
            if let ResolveError::MissingInput { site, context, .. } = &error {
                if let Some(removal) = supplied.removed.iter().find(|removal| {
                    removal.product.as_deref() == Some(&site.product)
                        && removal.entities.iter().all(|(dimension, value)| {
                            context.get(dimension) == Some(value)
                        })
                }) {
                    let origin = removal.origin.as_deref().unwrap_or("the recipe");
                    let origin = if origin.starts_with("line ") {
                        format!("recipe {origin}")
                    } else {
                        origin.to_owned()
                    };
                    message.push_str(&format!(
                        "\n  {} was excluded by {origin}; exclude the whole group or plan the rest with `--partial`",
                        removal.identity()
                    ));
                }
            }
            message.push_str(&format!(
                "\n  {} more artifacts cannot be produced; run `spit artifacts` to list them, or `spit dag --partial` to plan the rest",
                remaining.saturating_sub(1)
            ));
            diagnostics.push(within_call(
                Diagnostic::error(source, place, code_of("resolve", &error), message),
                &places,
                error_step(&error),
            ));
            return Err(error);
        }
        diagnostics.extend(path_warnings(pipeline, lines, &report.dag));
        Ok(report)
    });
    match resolved {
        Err(error) => {
            if !diagnostics.iter().any(Diagnostic::is_error) {
                let places = Places::new(pipeline, lines);
                let (source, place) = places.error_location(&error, records, true);
                diagnostics.push(within_call(
                    Diagnostic::error(source, place, code_of("resolve", &error), error.to_string()),
                    &places,
                    error_step(&error),
                ));
            }
            Err(diagnostics)
        }
        Ok(report) => {
            let produced: BTreeSet<&str> = report
                .dag
                .jobs
                .iter()
                .flat_map(|job| &job.outputs)
                .map(|&artifact| report.dag.artifact(artifact).product)
                .chain(
                    report
                        .incomplete
                        .iter()
                        .flat_map(|job| &job.outputs)
                        .map(|artifact| artifact.product.as_str()),
                )
                .collect();
            diagnostics.extend(empty_step_warnings(pipeline, lines, &produced, supplied));
            Ok((report, diagnostics))
        }
    }
}

fn finish(
    mut diagnostics: Vec<Diagnostic>,
    text: &str,
    source_text: Option<&str>,
) -> Vec<Diagnostic> {
    let pipeline_lines = Lines::new(text);
    let inventory_lines = source_text.map(Lines::new);
    let texts = [
        (DiagnosticSource::Pipeline, Some(&pipeline_lines)),
        (DiagnosticSource::Inventory, inventory_lines.as_ref()),
    ];
    for (source, lines) in texts {
        let Some(lines) = lines else { continue };
        for (index, line) in lines.iter().enumerate() {
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
                        && diagnostic.file.is_none()
                        && diagnostic.line == number
                })
                .collect();
            if errors.is_empty() {
                diagnostics.push(Diagnostic::new(
                    Severity::Warning,
                    source,
                    Some(Place::new(index + 1, columns.start..columns.end + 1)),
                    "comment.unknown-directive".to_owned(),
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
        let lines = match diagnostic.source {
            DiagnosticSource::Inventory => inventory_lines.as_ref().unwrap_or(&pipeline_lines),
            DiagnosticSource::Pipeline => &pipeline_lines,
        };
        diagnostic.columns = diagnostic
            .line
            .and_then(|line| lines.get(line))
            .map(content_columns);
    }
    order(diagnostics)
}

fn pipeline_diagnostics(
    document: &ParsedDocument,
    text: &str,
    inventory_text: &str,
) -> Vec<Diagnostic> {
    let (pipeline, lines) = (&document.pipeline, &document.lines);
    let checked = check_pipeline(document);
    let mut diagnostics = Vec::new();
    let mut errors = checked
        .definitions
        .errors
        .iter()
        .chain(&checked.rules)
        .peekable();
    if errors.peek().is_some() {
        let places = Places::new(pipeline, lines);
        diagnostics.extend(errors.map(|(subject, error)| {
            let diagnostic = |place| {
                Diagnostic::error(
                    DiagnosticSource::Pipeline,
                    place,
                    code_of("definition", error),
                    error.to_string(),
                )
            };
            if let DefinitionSubject::Call(call) = subject {
                let port = error_step(error).and_then(|(_, port)| port);
                return match places.at_call(*call, port) {
                    Some(call) => placed(diagnostic(None), call),
                    None => diagnostic(None),
                };
            }
            let place = places
                .subject_place(subject, error)
                .or_else(|| places.error_location(error, inventory_text, false).1);
            let step = match subject {
                DefinitionSubject::Invocation(output) => Some((
                    output.as_str(),
                    error_step(error).and_then(|(_, port)| port),
                )),
                _ => error_step(error),
            };
            within_call(diagnostic(place), &places, step)
        }));
    }
    let source = DiagnosticSource::Pipeline;
    diagnostics.extend(
        checked
            .commands
            .iter()
            .map(|error| Diagnostic::located("command", source, error, text)),
    );
    diagnostics.extend(
        checked
            .paths
            .iter()
            .map(|error| Diagnostic::located("path", source, error, text)),
    );
    diagnostics.extend(
        checked
            .warnings
            .iter()
            .map(|warning| pipeline_warning(warning, lines)),
    );
    if let Some(warning) = recipe::missing_root(pipeline.root.as_ref(), text) {
        diagnostics.push(warning);
    }
    diagnostics
}

fn within_call(
    diagnostic: Diagnostic,
    places: &Places<'_>,
    step: Option<(&str, Option<&str>)>,
) -> Diagnostic {
    match step.and_then(|(output, port)| places.in_call(output, port)) {
        Some(call) => placed(diagnostic, call),
        None => diagnostic,
    }
}

fn placed(mut diagnostic: Diagnostic, call: InCall) -> Diagnostic {
    diagnostic.source = DiagnosticSource::Pipeline;
    diagnostic.line = Some(call.place.line);
    diagnostic.columns = Some(call.place.columns);
    diagnostic.message.insert_str(0, &call.prefix);
    diagnostic.related = call.related;
    diagnostic
}

fn order(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    fn key(diagnostic: &Diagnostic) -> impl Ord + '_ {
        (
            diagnostic.source == DiagnosticSource::Inventory,
            diagnostic.file.as_deref(),
            diagnostic.line.is_none(),
            diagnostic.line,
            diagnostic.severity,
        )
    }
    diagnostics.sort_by(|a, b| key(a).cmp(&key(b)));
    type Reported<'a> = (&'a str, Option<Range<usize>>);
    let mut errored: BTreeMap<_, Vec<Reported>> = BTreeMap::new();
    let keep: Vec<bool> = diagnostics
        .iter()
        .map(|diagnostic| {
            let Some(line) = diagnostic.line else {
                return true;
            };
            let place = (diagnostic.source, diagnostic.file.as_deref(), line);
            let spans = errored.entry(place).or_default();
            if !diagnostic.is_error() {
                return spans.is_empty();
            }
            let columns = &diagnostic.columns;
            let repeats = spans.iter().any(|(message, span)| {
                *message == diagnostic.message
                    && (span.is_none() || columns.is_none() || span == columns)
            });
            if !repeats {
                spans.push((&diagnostic.message, columns.clone()));
            }
            !repeats
        })
        .collect();
    let mut keep = keep.into_iter();
    diagnostics.retain(|_| keep.next().unwrap_or(true));
    diagnostics
}
