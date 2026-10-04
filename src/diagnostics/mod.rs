//! Editor-friendly validation of an in-memory SPIT document.

mod diagnostic;
mod places;
mod recovery;
mod warnings;

pub use diagnostic::*;
pub(crate) use recovery::recover_document;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::command::collect_commands;
use crate::compile::collect_pipeline;
use crate::imports::parse_located_document_recovering;
use crate::inputs::{
    check_inventory, collect_exclusion_errors, collect_rule_errors, with_source_paths, InputCheck,
    InputError, InputSpec, ResolvedInputs,
};
use crate::lower::{parse_document_recovering, ParsedDocument};
use crate::model::{ArtifactReport, CoverageGap, InputRules, PipelineIndex, SourceInventory};
use crate::parser::{as_read_back, glued_comment, without_bom, Kind, Rule, SourceMap};
use crate::paths::{collect_paths, shown_path, PathTemplate};
use crate::resolver::first_failure;
use crate::span::{content_columns, Lines, Place};
use crate::{
    parse_source_inventory, resolve_artifacts_excluding, DefinitionSubject, ParseError, Pipeline,
    ResolveError,
};

use places::{error_location, error_step, in_call, subject_place};
use recovery::recover_parse_errors;
use warnings::{case_warnings, empty_step_warnings, label_warnings, near_miss_warnings, warnings};

/// Where a pipeline is, and what applies to it, when diagnosing it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    /// The pipeline's file, so that its `use` lines can be resolved. Without
    /// one, a `use` line is an error.
    pub path: Option<&'a Path>,
    /// A `.spitin` recipe whose rules and source paths are checked with the
    /// pipeline.
    pub recipe: Option<&'a InputSpec>,
    /// Report records that leave jobs unmade without failing, as
    /// `spit artifacts` does.
    pub lenient: bool,
}

impl<'a> Context<'a> {
    /// The pipeline in the file at `path`.
    pub fn at(path: &'a Path) -> Self {
        Self {
            path: Some(path),
            ..Self::default()
        }
    }

    /// Parse a pipeline in this context: with its imports resolved from its
    /// path, and its recipe checked and applied.
    fn parse(&self, text: &str) -> Result<Parsed, Vec<ParseError>> {
        let mut document = match self.path {
            Some(path) => parse_located_document_recovering(text, path, Kind::Pipeline)?,
            None => parse_document_recovering(text, &BTreeMap::new(), Kind::Pipeline)?,
        };
        let Some(recipe) = self.recipe else {
            return Ok(Parsed {
                document,
                as_written: None,
            });
        };
        recipe
            .check(&document.pipeline)
            .map_err(|error| vec![ParseError::new(1, error.to_string())])?;
        let as_written = document.pipeline.clone();
        recipe.apply_paths(&mut document.pipeline);
        document.inputs = recipe.rules.clone();
        Ok(Parsed {
            document,
            as_written: Some(as_written),
        })
    }
}

/// A parsed pipeline, and the pipeline as written when a recipe added its
/// source paths.
struct Parsed {
    document: ParsedDocument,
    as_written: Option<Pipeline>,
}

impl Parsed {
    /// The document checked with `warnings`, its pipeline as written.
    fn checked(self, warnings: Vec<Diagnostic>) -> Checked {
        let paths = shown_paths(&self.document.pipeline, &self.document.lines);
        Checked {
            pipeline: self.as_written.unwrap_or(self.document.pipeline),
            warnings,
            paths,
        }
    }
}

/// The paths an editor shows: each product's, where it comes from a default
/// rule or gains an extension, so is written out nowhere. Outputs with no
/// rule at all show the built-in default they are given.
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
                // A source with a default rule is shown on its declaration.
                None => lines.products.get(&product.name)?.line,
            };
            Some(ShownPath {
                product: product.name.clone(),
                line,
                path: shown_path(&index, &product.name, template.as_deref()?),
            })
        })
        .collect();
    shown.sort_by_key(|path| path.line);
    shown
}

/// Collect independent syntax errors throughout the document, then check its
/// semantics once the document parses: every declaration, step, rule,
/// command, and path error, and warnings for likely mistakes. Jobs are
/// resolved against the inventory only once nothing else is wrong. An absent
/// inventory still allows every other check. Diagnostics are ordered by
/// line, with at most one error per line.
pub fn diagnose(text: &str, source_text: Option<&str>) -> Vec<Diagnostic> {
    diagnose_in(text, source_text, Context::default())
}

/// As [`diagnose`], in `context`.
pub fn diagnose_in(text: &str, source_text: Option<&str>, context: Context<'_>) -> Vec<Diagnostic> {
    let diagnosis = match source_text {
        None => diagnose_checked(text, context),
        Some(records) => {
            diagnose_checked_with_records(text, records, context).map(|(checked, _)| checked)
        }
    };
    diagnosis.map_or_else(|all| all, |checked| checked.warnings)
}

/// Diagnose a pipeline without records, and return it once it passes: every
/// parse error, or once it parses, every declaration, step, rule, command
/// and path. The pipeline is returned as written, without the source paths
/// a recipe adds.
pub fn diagnose_checked(text: &str, context: Context<'_>) -> Diagnosis {
    let text = without_bom(text);
    let parsed = recover_parse_errors(text, |text| context.parse(text)).map_err(|errors| {
        let diagnostics = located_all(DiagnosticSource::Pipeline, errors, text);
        finish(diagnostics.collect(), text, None)
    })?;
    let warnings = check_document(&parsed.document, text, None)?;
    Ok(parsed.checked(finish(warnings, text, None)))
}

/// As [`diagnose_checked`], with the records in `records` settled and
/// resolved over the pipeline once it passes on its own; the inventory they
/// parse to, and what it resolves to, are returned with it.
///
/// Keep in step with [`diagnose_checked_with_inventory`], which takes the
/// same steps over records in memory: a step added or changed here must be
/// added or changed there, or recipes will be diagnosed differently when
/// they pass than when they fail.
pub fn diagnose_checked_with_records(
    text: &str,
    records: &str,
    context: Context<'_>,
) -> Diagnosis<(Checked, Records)> {
    let (text, records) = (without_bom(text), without_bom(records));
    let parsed = (
        recover_parse_errors(text, |text| context.parse(text)),
        recover_parse_errors(records, |records| {
            parse_source_inventory(records).map_err(|error| vec![error])
        }),
    );
    let (parsed, inventory) = match parsed {
        (Ok(parsed), Ok(inventory)) => (parsed, inventory),
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
            return Err(finish(diagnostics, text, Some(records)));
        }
    };
    let document = &parsed.document;
    let mut diagnostics = check_document(document, text, Some(records))?;
    let report = record_diagnostics(document, &inventory, None, records, context.lenient);
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
    let diagnostics = finish(diagnostics, text, Some(records));
    match report {
        Some(report) if !diagnostics.iter().any(Diagnostic::is_error) => {
            Ok((parsed.checked(diagnostics), Records { inventory, report }))
        }
        _ => Err(diagnostics),
    }
}

/// As [`diagnose_checked_with_records`], for the records `settled` in memory
/// by [`InputSpec::resolve`] with the context's recipe, without writing them
/// as text and reading them back. It gives `None`, so that the caller
/// diagnoses their text instead, when a diagnostic would point into that
/// text: when anything is an error or concerns the records, or when a path
/// rule the text would carry holds a `#`.
///
/// Keep in step with [`diagnose_checked_with_records`]: this takes its steps
/// but parsing the records, and must give what it gives whenever this gives
/// anything. `tests/outputs.rs` compares the two.
pub fn diagnose_checked_with_inventory(
    text: &str,
    settled: &ResolvedInputs,
    context: Context<'_>,
) -> Option<(Checked, Records)> {
    let text = without_bom(text);
    let parsed = recover_parse_errors(text, |text| context.parse(text)).ok()?;
    let document = &parsed.document;
    let no_rules = InputRules::default();
    let rules = context.recipe.map_or(&no_rules, |recipe| &recipe.rules);
    let inventory = as_read_back(&settled.inventory, &document.pipeline, rules)?;
    // Checking the settled records again finds what settling found, since
    // only the conditional `exclude` rules change records and they ran while settling.
    let gaps = Some(settled.gaps.as_slice());
    // Only errors are placed in the records' text.
    let mut diagnostics = check_document(document, text, None).ok()?;
    let lenient = context.lenient;
    let (report, found) = record_diagnostics(document, &inventory, gaps, "", lenient).ok()?;
    diagnostics.extend(found);
    let diagnostics = finish(diagnostics, text, None);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error() || diagnostic.source == DiagnosticSource::Inventory)
    {
        return None;
    }
    Some((parsed.checked(diagnostics), Records { inventory, report }))
}

/// Diagnose a `.spitout` on its own: the syntax of its records. Whether the
/// sources they name are a pipeline's is checked by `dag` and `artifacts`.
pub fn diagnose_inputs(text: &str) -> Vec<Diagnostic> {
    let text = without_bom(text);
    match parse_source_inventory(text) {
        Ok(_) => Vec::new(),
        Err(parse) => {
            let diagnostic = Diagnostic::located(DiagnosticSource::Inventory, &parse, text);
            finish(vec![diagnostic], text, Some(text))
        }
    }
}

/// Diagnose a `.spitin` recipe at `path` without reading any data: its own
/// lines, then the pipeline its `pipeline` line names, then its rules and
/// any records against that pipeline. The pipeline's own errors are named
/// by file and line, since they are not in `text`.
pub fn diagnose_recipe(text: &str, path: &Path) -> Vec<Diagnostic> {
    let text = without_bom(text);
    let error = |message: String| Diagnostic::error(DiagnosticSource::Pipeline, None, message);
    let spec = match crate::inputs::parse_input_spec_at(text, path) {
        Ok(spec) => spec,
        Err(parse) => {
            let diagnostic = Diagnostic::located(DiagnosticSource::Pipeline, &parse, text);
            return finish(vec![diagnostic], text, None);
        }
    };
    let Some(pipeline_path) = spec.pipeline else {
        let message =
            "name the pipeline this recipe is for, with a line such as `pipeline analysis.spit`";
        return finish(vec![error(message.to_owned())], text, None);
    };
    let shown = pipeline_path.display().to_string();
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
    match diagnose_checked(&pipeline_text, Context::at(&pipeline_path)) {
        Ok(checked) => {
            let mut diagnostics = diagnose_recipe_against(text, &checked.pipeline);
            // The rows of files `exclude from` lines name are read with the
            // recipe at its path, not with its text; each error says where.
            let rows = InputRules {
                exclusions: spec
                    .rules
                    .exclusions
                    .iter()
                    .filter(|rule| !rule.origin.starts_with("line "))
                    .cloned()
                    .collect(),
                ..InputRules::default()
            };
            diagnostics.extend(
                collect_exclusion_errors(&PipelineIndex::new(&checked.pipeline), &rows)
                    .into_iter()
                    .map(|(_, problem)| error(problem.to_string())),
            );
            if let Some(warning) = missing_root(spec.root.as_ref(), text) {
                diagnostics.push(warning);
            }
            diagnostics
        }
        Err(diagnostics) => {
            let external: Arc<str> = Arc::from(pipeline_text.as_str());
            let pipeline_errors = diagnostics
                .into_iter()
                .filter(Diagnostic::is_error)
                .map(|mut diagnostic| {
                    // A library's path is from the pipeline's folder.
                    let folder = pipeline_path.parent().unwrap_or_else(|| Path::new(""));
                    match &mut diagnostic.file {
                        // An error in a library keeps its own file and text.
                        Some(file) => *file = folder.join(&*file).display().to_string(),
                        None => {
                            diagnostic.file = Some(shown.clone());
                            diagnostic.external_text = Some(Arc::clone(&external));
                        }
                    }
                    for related in &mut diagnostic.related {
                        if let Some(file) = &mut related.file {
                            *file = folder.join(&*file).display().to_string();
                        }
                    }
                    diagnostic
                })
                .collect();
            finish(pipeline_errors, text, None)
        }
    }
}

/// A warning on the recipe's `root` line when the folder it names is not
/// there, which `spit check` can say without reading any data. Not an
/// error: the recipe may be checked on one machine and run on another,
/// where the folder is; `spit inputs` stops on it where it runs.
fn missing_root(root: Option<&(PathBuf, usize)>, text: &str) -> Option<Diagnostic> {
    let (root, line) = root?;
    if root.is_dir() {
        return None;
    }
    let written = text.lines().nth(line - 1)?;
    let folder = crate::parser::strip_comment(written)
        .trim()
        .strip_prefix("root ")?
        .trim();
    let start = written.find(folder)?;
    let message = format!(
        "dataset root `{folder}` is not a folder here; `spit inputs` and `spit dag` stop with an error unless it is one where they run"
    );
    Some(Diagnostic::new(
        Severity::Warning,
        DiagnosticSource::Pipeline,
        Some(Place::new(*line, start..start + folder.len())),
        message,
    ))
}

/// Diagnose the text of a `.spitin` recipe against `pipeline`, reading no
/// data: every rule's error at the rule, then its source paths and records.
pub fn diagnose_recipe_against(text: &str, pipeline: &Pipeline) -> Vec<Diagnostic> {
    let text = without_bom(text);
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
                DefinitionSubject::Exclusion(index) => lines.exclusions.get(*index).cloned(),
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
            // A recipe without records is scanned, which finds every
            // source by its rule.
            None => spec
                .check(pipeline)
                .and_then(|()| match spec.source_without_path(pipeline) {
                    Some(product) => Err(InputError::NoSourcePath { product }),
                    None => Ok(()),
                }),
        };
        if let Err(problem) = checked {
            // Records written in the recipe keep its line numbers, and a
            // path the recipe should not give is at its `path` line.
            let place = match &problem {
                InputError::Resolve(error) => {
                    error_location(pipeline, &lines, error, text, false).1
                }
                InputError::NotASource { product }
                | InputError::OutputPath { product }
                | InputError::PathInBoth { product }
                | InputError::MemberPath { product, .. } => lines.paths.get(product).cloned(),
                _ => None,
            };
            diagnostics.push(Diagnostic::error(
                DiagnosticSource::Pipeline,
                place,
                problem.to_string(),
            ));
        }
    }
    if diagnostics.is_empty() {
        diagnostics = recipe_path_errors(pipeline, &spec, &lines, text);
    }
    finish(diagnostics, text, None)
}

/// Errors in the recipe's source path rules, its default's included, each
/// at the recipe line that writes the rule. An error placed at one of the
/// pipeline's rules, such as a collision with an output, has no place.
fn recipe_path_errors(
    pipeline: &Pipeline,
    spec: &InputSpec,
    lines: &SourceMap,
    text: &str,
) -> Vec<Diagnostic> {
    let source_paths = spec.rules.source_paths_for(pipeline);
    if source_paths.is_empty() {
        return Vec::new();
    }
    let merged = with_source_paths(pipeline, &source_paths);
    let mut places = SourceMap::default();
    for name in source_paths.keys() {
        if let Some(place) = lines.paths.get(name).or(lines.default_path.as_ref()) {
            places.paths.insert(name.clone(), place.clone());
        }
    }
    collect_paths(&merged, &places, &BTreeSet::new())
        .1
        .iter()
        .map(|error| Diagnostic::located(DiagnosticSource::Pipeline, error, text))
        .collect()
}

/// Each parse error in `text` as a diagnostic.
fn located_all<'a>(
    source: DiagnosticSource,
    errors: impl IntoIterator<Item = ParseError> + 'a,
    text: &'a str,
) -> impl Iterator<Item = Diagnostic> + 'a {
    errors
        .into_iter()
        .map(move |error| Diagnostic::located(source, &error, text))
}

/// Check a parsed pipeline on its own: its warnings, or every diagnostic,
/// finished, when one is an error. Errors about records point into
/// `source_text` when given.
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

/// Settle `supplied` with the input stage, then resolve jobs over it: what
/// they resolve to with any warnings, about paths that differ only in case
/// and steps that make nothing, or those diagnostics with the error that
/// stops either.
/// `settled` holds what settling `supplied` with the same rules found:
/// checking it again would find the same, and change nothing. Keep in step
/// with `check_inventory`: this holds only while checking changes no
/// records, which conditional `exclude` rules do only while the inventory is settled.
fn record_diagnostics(
    document: &ParsedDocument,
    supplied: &SourceInventory,
    settled: Option<&[CoverageGap]>,
    records: &str,
    lenient: bool,
) -> Result<(ArtifactReport, Vec<Diagnostic>), Vec<Diagnostic>> {
    let (pipeline, lines) = (&document.pipeline, &document.lines);
    let mut diagnostics = Vec::new();
    let checked = match settled {
        Some(gaps) => Ok(InputCheck {
            inventory: Cow::Borrowed(supplied),
            gaps: gaps.to_vec(),
        }),
        None => check_inventory(pipeline, &document.inputs, Cow::Borrowed(supplied)),
    };
    let resolved = checked.and_then(|checked| {
        if lenient {
            let unavailable: Vec<_> = checked
                .gaps
                .iter()
                .flat_map(|gap| gap.sources.iter().cloned())
                .collect();
            let report = resolve_artifacts_excluding(pipeline, &checked.inventory, &unavailable)?;
            diagnostics.extend(near_miss_warnings(&report));
            return Ok(report);
        }
        if let Some(gap) = checked.gaps.into_iter().next() {
            return Err(gap.error);
        }
        let report = resolve_artifacts_excluding(pipeline, &checked.inventory, &[])?;
        diagnostics.extend(near_miss_warnings(&report));
        if let Some(error) = first_failure(&report.incomplete) {
            let remaining: usize = report.incomplete.iter().map(|job| job.outputs.len()).sum();
            let (source, place) = error_location(pipeline, lines, &error, records, true);
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
                Diagnostic::error(source, place, message),
                pipeline,
                lines,
                error_step(&error),
            ));
            return Err(error);
        }
        diagnostics.extend(case_warnings(pipeline, lines, &report.dag));
        diagnostics.extend(label_warnings(pipeline, lines, &report.dag));
        Ok(report)
    });
    match resolved {
        Err(error) => {
            if !diagnostics.iter().any(Diagnostic::is_error) {
                let (source, place) = error_location(pipeline, lines, &error, records, true);
                diagnostics.push(within_call(
                    Diagnostic::error(source, place, error.to_string()),
                    pipeline,
                    lines,
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

/// Flag each `#` that reads like a comment but is part of a word, give every
/// diagnostic with a line its columns, then order.
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
            let step = match subject {
                DefinitionSubject::Invocation(output) => Some((
                    output.as_str(),
                    error_step(error).and_then(|(_, port)| port),
                )),
                _ => error_step(error),
            };
            within_call(
                Diagnostic::error(DiagnosticSource::Pipeline, place, error.to_string()),
                pipeline,
                lines,
                step,
            )
        })
        .collect();
    let source = DiagnosticSource::Pipeline;
    let command_errors = collect_commands(pipeline, lines, &checked.poisoned);
    let path_errors = collect_paths(pipeline, lines, &checked.poisoned).1;
    diagnostics.extend(
        command_errors
            .iter()
            .map(|error| Diagnostic::located(source, error, text)),
    );
    diagnostics.extend(
        path_errors
            .iter()
            .map(|error| Diagnostic::located(source, error, text)),
    );
    diagnostics.extend(warnings(pipeline, lines, &checked.poisoned));
    diagnostics
}

/// `diagnostic`, about the step making a product at a port, reported at the
/// call written in the text when a call to an operation carried out by
/// steps made that step; see [`in_call`].
fn within_call(
    mut diagnostic: Diagnostic,
    pipeline: &Pipeline,
    lines: &SourceMap,
    step: Option<(&str, Option<&str>)>,
) -> Diagnostic {
    let Some(call) = step.and_then(|(output, port)| in_call(pipeline, lines, output, port)) else {
        return diagnostic;
    };
    diagnostic.source = DiagnosticSource::Pipeline;
    diagnostic.line = Some(call.place.line);
    diagnostic.columns = Some(call.place.columns);
    diagnostic.message.insert_str(0, &call.prefix);
    diagnostic.related = call.related;
    diagnostic
}

/// Order by source and line, keep only the first error on each line, and
/// drop warnings on a line that already has an error.
fn order(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    // Those in the text diagnosed first, then each other file's.
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
    let mut errored = BTreeSet::new();
    let keep: Vec<bool> = diagnostics
        .iter()
        .map(|diagnostic| {
            let Some(line) = diagnostic.line else {
                return true;
            };
            let place = (diagnostic.source, diagnostic.file.as_deref(), line);
            if errored.contains(&place) {
                return false;
            }
            if diagnostic.is_error() {
                errored.insert(place);
            }
            true
        })
        .collect();
    let mut keep = keep.into_iter();
    diagnostics.retain(|_| keep.next().unwrap_or(true));
    diagnostics
}
