//! Editor-friendly validation of an in-memory SPIT document.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::command::collect_commands;
use crate::compile::collect_pipeline;
use crate::imports::parse_located_document;
use crate::inputs::{
    check_inventory, collect_exclusion_errors, collect_rule_errors, InputCheck, InputError,
    InputSpec, ResolvedInputs,
};
use crate::json::Json;
use crate::lower::{parse_document_with_imports, ParsedDocument};
use crate::model::DEFAULT_OUTPUT;
use crate::model::{
    stage_within, ArtifactReport, CommandRole, CoverageGap, InputRules, PipelineIndex, ResolvedDag,
    SourceInventory,
};
use crate::parser::{
    as_read_back, glued_comment, source_record_lines, without_bom, Kind, Rule, SourceMap, Step,
};
use crate::paths::{case_collisions, collect_paths, dashed_labels, shown_path, PathTemplate};
use crate::resolver::first_failure;
use crate::span::{content_columns, utf16_columns, Located, Place};
use crate::{
    parse_source_inventory, resolve_artifacts_excluding, DefinitionSubject, EntityBinding,
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
    /// A different file from the document being checked, when a recipe's
    /// pipeline contains the error.
    pub file: Option<String>,
    /// Text of that file, used to convert its byte columns to UTF-16.
    pub external_text: Option<String>,
}

/// A document that passed every check: its pipeline, and its warnings.
#[derive(Debug)]
pub struct Checked {
    pub pipeline: Pipeline,
    /// Every diagnostic found, none of them an error, in line order.
    pub warnings: Vec<Diagnostic>,
    /// The path of each product whose path is not written out on a line of
    /// its own, in line order, for an editor to show.
    pub paths: Vec<ShownPath>,
}

/// A product's path as its rules give it, `{@product}` and `{@stage}` written
/// out, and the line that declares the product: a step's, or a source's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShownPath {
    pub product: String,
    pub line: usize,
    pub path: String,
}

/// Records that passed diagnosis with their pipeline: the inventory they
/// parse to, and what it resolves to, so a caller need not resolve it again.
#[derive(Debug)]
pub struct Records {
    pub inventory: SourceInventory,
    /// The jobs over the records' sources, with those that cannot be made in
    /// a lenient diagnosis. Sources take their files from the records, so a
    /// caller that locates them later gives them their files with
    /// [`ResolvedDag::locate_sources`].
    pub report: ArtifactReport,
}

/// What diagnosing a document found: what it checked to, or, when any
/// diagnostic is an error, every diagnostic in line order.
pub type Diagnosis<T = Checked> = Result<T, Vec<Diagnostic>>;

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
            file: None,
            external_text: None,
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
            file: None,
            external_text: None,
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
        self.display_named(text, source_text, FileNames::default())
    }

    /// As [`Diagnostic::display_in`], naming the file the diagnostic is in
    /// when `names` gives one: `error: pipeline.spit: line 5, column 12:
    /// message`. A caller names a file that is not the one its user gave,
    /// such as the pipeline a recipe names.
    pub fn display_named<'a>(
        &'a self,
        text: &'a str,
        source_text: Option<&'a str>,
        names: FileNames<'a>,
    ) -> impl fmt::Display + 'a {
        DisplayIn {
            diagnostic: self,
            column: self.column_in(text, source_text),
            name: self.name_in(names),
        }
    }

    /// The 1-based column in characters, given the texts that were diagnosed.
    fn column_in(&self, text: &str, source_text: Option<&str>) -> Option<usize> {
        let columns = self.columns.as_ref()?;
        let line = self.line_text(text, source_text)?;
        Some(line.get(..columns.start)?.chars().count() + 1)
    }

    /// The name `names` gives the text this diagnostic is in.
    fn name_in<'a>(&'a self, names: FileNames<'a>) -> Option<&'a str> {
        if let Some(file) = &self.file {
            return Some(file);
        }
        match self.source {
            DiagnosticSource::Pipeline => names.pipeline,
            DiagnosticSource::Inventory => names.inventory,
        }
    }

    /// The diagnosed line, from the pipeline text or the separate inventory.
    fn line_text<'a>(&'a self, text: &'a str, source_text: Option<&'a str>) -> Option<&'a str> {
        let text = match self.source {
            DiagnosticSource::Inventory => source_text?,
            DiagnosticSource::Pipeline => self.external_text.as_deref().unwrap_or(text),
        };
        without_bom(text).lines().nth(self.line?.checked_sub(1)?)
    }

    fn write(
        &self,
        f: &mut fmt::Formatter<'_>,
        column: Option<usize>,
        name: Option<&str>,
    ) -> fmt::Result {
        write!(f, "{}: ", self.severity.as_str())?;
        self.write_located(f, column, name)
    }

    /// The diagnostic's place, then its message. A named file leads the
    /// place; an unnamed inventory is called `inventory`. A diagnostic about
    /// the pipeline with no line, such as a recipe rule's coverage gap, is
    /// about no line of the pipeline, so the pipeline is not named.
    fn write_located(
        &self,
        f: &mut fmt::Formatter<'_>,
        column: Option<usize>,
        name: Option<&str>,
    ) -> fmt::Result {
        let inventory = self.source == DiagnosticSource::Inventory;
        let name = name.filter(|_| inventory || self.line.is_some());
        match (name, self.line) {
            (Some(name), _) => write!(f, "{name}: ")?,
            (None, Some(_)) if inventory => f.write_str("inventory ")?,
            (None, None) if inventory => f.write_str("inventory: ")?,
            (None, _) => {}
        }
        match (self.line, column) {
            (Some(line), Some(column)) => write!(f, "line {line}, column {column}: ")?,
            (Some(line), None) => write!(f, "line {line}: ")?,
            (None, _) => {}
        }
        f.write_str(&self.message)
    }
}

/// The files a diagnostic's texts came from, for those a message should
/// name: the pipeline, or the main text diagnosed, and the inventory.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileNames<'a> {
    pub pipeline: Option<&'a str>,
    pub inventory: Option<&'a str>,
}

/// A diagnostic rendered with its column; see [`Diagnostic::display_in`].
struct DisplayIn<'a> {
    diagnostic: &'a Diagnostic,
    column: Option<usize>,
    name: Option<&'a str>,
}

impl fmt::Display for DisplayIn<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic.write(f, self.column, self.name)
    }
}

/// Reads as `error: line 3: message`, naming the inventory for its lines.
/// Without the diagnosed text a column cannot be counted in characters; use
/// [`Diagnostic::display_in`] to include it.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, None, None)
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
    format!(
        "{}\n",
        Json::object([(
            "diagnostics",
            diagnostics_json(diagnostics, text, source_text)
        )])
    )
}

/// As [`render_diagnostics_json`] for a pipeline that checked clean, with
/// the `paths` an editor shows beside each product's line.
pub fn render_check_json(diagnostics: &[Diagnostic], text: &str, paths: &[ShownPath]) -> String {
    format!(
        "{}\n",
        Json::object([
            ("diagnostics", diagnostics_json(diagnostics, text, None)),
            ("paths", shown_paths_json(paths)),
        ])
    )
}

pub(crate) fn shown_paths_json(paths: &[ShownPath]) -> Json<'_> {
    Json::array(paths.iter().map(|path| {
        Json::object([
            ("product", Json::string(&path.product)),
            ("line", Json::number_or_null(Some(path.line))),
            ("path", Json::string(&path.path)),
        ])
    }))
}

pub(crate) fn diagnostics_json<'a>(
    diagnostics: &'a [Diagnostic],
    text: &str,
    source_text: Option<&str>,
) -> Json<'a> {
    let items = diagnostics.iter().map(|diagnostic| {
        let columns = diagnostic.utf16_columns(text, source_text);
        let mut fields = vec![
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
        ];
        if let Some(file) = &diagnostic.file {
            fields.push(("file", Json::string(file)));
        }
        Json::object(fields)
    });
    Json::array(items)
}

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
    fn parse(&self, text: &str) -> Result<Parsed, ParseError> {
        let mut document = match self.path {
            Some(path) => parse_located_document(text, path, Kind::Pipeline)?,
            None => parse_document_with_imports(text, &BTreeMap::new(), Kind::Pipeline)?,
        };
        let Some(recipe) = self.recipe else {
            return Ok(Parsed {
                document,
                as_written: None,
            });
        };
        recipe
            .check(&document.pipeline)
            .map_err(|error| ParseError::new(1, error.to_string()))?;
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
    let mut defaulted;
    let pipeline = if pipeline.path_template.is_none() {
        defaulted = pipeline.clone();
        defaulted.path_template = Some(PathTemplate::default_output());
        &defaulted
    } else {
        pipeline
    };
    let index = PipelineIndex::new(pipeline);
    let mut shown: Vec<_> = pipeline
        .products
        .iter()
        .filter(|product| !lines.imported.contains(&product.name))
        .filter(|product| {
            !pipeline.product_paths.contains_key(&product.name)
                || index.added_extension(&product.name).is_some()
                || index
                    .path_rule_for(&product.name)
                    .is_some_and(PathTemplate::varies)
        })
        .filter_map(|product| {
            let line = match lines.invocations.get(&product.name) {
                Some(step) => step.line,
                // A source with a default rule is shown on its declaration.
                None => lines.products.get(&product.name)?.line,
            };
            Some(ShownPath {
                product: product.name.clone(),
                line,
                path: shown_path(&index, &product.name)?,
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
        recover_parse_errors(records, parse_source_inventory),
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
    let inventory = as_read_back(&settled.inventory, rules)?;
    // Checking the settled records again finds what settling found, since
    // only the `drop` rules change records and they ran while settling.
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
                collect_exclusion_errors(&checked.pipeline, &rows)
                    .into_iter()
                    .map(|(_, problem)| error(problem.to_string())),
            );
            if let Some(warning) = missing_root(spec.root.as_ref(), text) {
                diagnostics.push(warning);
            }
            diagnostics
        }
        Err(diagnostics) => {
            let pipeline_errors = diagnostics
                .into_iter()
                .filter(Diagnostic::is_error)
                .map(|mut diagnostic| {
                    diagnostic.file = Some(shown.clone());
                    diagnostic.external_text = Some(pipeline_text.clone());
                    diagnostic
                })
                .collect();
            finish(pipeline_errors, text, None)
        }
    }
}

/// Diagnose the text of a `.spitin` recipe against `pipeline`, reading no
/// data: every rule's error at the rule, then its source paths and records.
/// A warning on the recipe's `root` line when the folder it names is not
/// there, which `spit check` can say without reading any data.
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
        "dataset root `{folder}` is not a folder; `spit inputs` and `spit dag` will find no files there"
    );
    Some(Diagnostic::new(
        Severity::Warning,
        DiagnosticSource::Pipeline,
        Some(Place::new(*line, start..start + folder.len())),
        message,
    ))
}

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
            None => spec.check(pipeline),
        };
        if let Err(problem) = checked {
            // Records written in the recipe keep its line numbers.
            let place = match &problem {
                InputError::Resolve(error) => {
                    error_location(pipeline, &lines, error, text, false).1
                }
                _ => None,
            };
            diagnostics.push(Diagnostic::error(
                DiagnosticSource::Pipeline,
                place,
                problem.to_string(),
            ));
        }
    }
    finish(diagnostics, text, None)
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
/// records, which `drop` rules do only while the inventory is settled.
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
            diagnostics.push(Diagnostic::error(source, place, message));
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
                diagnostics.push(Diagnostic::error(source, place, error.to_string()));
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

/// Unused sources that spell a value nearly as an incomplete job needs it.
fn near_miss_warnings(report: &ArtifactReport) -> Vec<Diagnostic> {
    let unused: BTreeSet<_> = report
        .unused_sources()
        .into_iter()
        .map(|id| report.dag.artifact(id).to_instance().key())
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
            let key = near.artifact.key();
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

/// Flag paths that differ only in case, which are one file on macOS and Windows.
fn case_warnings(pipeline: &Pipeline, lines: &SourceMap, dag: &ResolvedDag) -> Vec<Diagnostic> {
    let index = PipelineIndex::new(pipeline);
    case_collisions(pipeline, dag)
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
        .collect()
}

/// Flag values with a `-` that `{@labels}` writes, as `sub-01-a`, which
/// BIDS reads as a new entity.
fn label_warnings(pipeline: &Pipeline, lines: &SourceMap, dag: &ResolvedDag) -> Vec<Diagnostic> {
    let index = PipelineIndex::new(pipeline);
    dashed_labels(pipeline, dag)
        .into_iter()
        .map(|(product, dimension, value)| {
            warning(
                lines.path_rule(&index, &product),
                format!(
                    "`{{@labels}}` writes `{dimension}-{value}` in the path of `{product}`; BIDS reads each `-` as the end of a key, so it cannot read `{value}` back"
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
                    Place::new(place.line, columns.unwrap_or_else(|| place.columns.clone()))
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
    warnings.extend(name_warnings(pipeline, lines));
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

/// Products named after the operation that makes them, as in
/// `digest = digest(log)`: legal, since the names are separate, but the step
/// then reads as the operation, not its result.
fn name_warnings(pipeline: &Pipeline, lines: &SourceMap) -> Vec<Diagnostic> {
    pipeline
        .invocations
        .iter()
        .flat_map(|invocation| {
            invocation
                .outputs
                .iter()
                .filter(move |output| **output == invocation.operation)
        })
        .map(|output| {
            warning(
                lines.invocations.get(output).map(Step::output),
                format!(
                    "product `{output}` has the name of the operation that makes it; \
                     name the result instead, so the step reads as what it makes"
                ),
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

fn empty_step_warnings(
    pipeline: &Pipeline,
    lines: &SourceMap,
    produced: &BTreeSet<&str>,
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
) -> Result<T, Vec<ParseError>> {
    let (parsed, errors) = recover_document(text, parse);
    match parsed {
        Some(parsed) if errors.is_empty() => Ok(parsed),
        _ => Err(errors),
    }
}

/// Keep the independently parseable declarations for editor information,
/// while retaining every error for callers that require a valid document.
pub(crate) fn recover_document<T>(
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
                if let ParseErrorKind::MisplacedRecords { lines: records } = error.kind() {
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
    let ParseErrorKind::UndeclaredOperation { name: operation } = error.kind() else {
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
