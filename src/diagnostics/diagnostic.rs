//! A diagnostic: what is wrong or doubtful, in which text and where, and
//! how it is written as text or JSON.

use std::fmt;
use std::ops::Range;

use crate::json::Json;
use crate::model::{ArtifactReport, SourceInventory};
use crate::parser::without_bom;
use crate::span::{utf16_columns, Located, Place};
use crate::Pipeline;

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
    /// Other places the diagnostic is about, such as the step in a
    /// library's body that a call it is reported at made.
    pub related: Vec<Related>,
}

/// A place a diagnostic is also about, with what it is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Related {
    /// The file it is in, from the pipeline's folder; `None` for the file
    /// of the diagnostic's own line.
    pub file: Option<String>,
    pub line: usize,
    /// The byte range within the line.
    pub columns: Range<usize>,
    /// The line itself, to count its columns in characters or UTF-16 code
    /// units; `None` for a line of the diagnostic's own file.
    pub line_text: Option<String>,
    pub message: String,
}

impl Related {
    /// The line's text, from `text`, the text of the diagnostic's own line,
    /// when it is in it.
    fn line_in<'a>(&'a self, text: &'a str) -> Option<&'a str> {
        match &self.line_text {
            Some(line) => Some(line),
            None => without_bom(text).lines().nth(self.line.checked_sub(1)?),
        }
    }
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
    pub(super) fn new(
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
            related: Vec::new(),
        }
    }

    /// An error that records its own location, narrowed to what it is about
    /// within `text`, the text it is in.
    pub(super) fn located<E: fmt::Display>(
        source: DiagnosticSource,
        error: &Located<E>,
        text: &str,
    ) -> Self {
        let place = error.location.place_in(text);
        Self {
            severity: Severity::Error,
            source,
            line: error.location.line,
            columns: place.map(|place| place.columns),
            message: error.message(),
            file: None,
            external_text: None,
            related: Vec::new(),
        }
    }

    pub(super) fn error(source: DiagnosticSource, place: Option<Place>, message: String) -> Self {
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
            text,
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
        self.write_with(f, column, name, &self.message)
    }

    /// As [`Diagnostic::write`], with `message` in place of its own.
    fn write_with(
        &self,
        f: &mut fmt::Formatter<'_>,
        column: Option<usize>,
        name: Option<&str>,
        message: &str,
    ) -> fmt::Result {
        write!(f, "{}: ", self.severity.as_str())?;
        self.write_located(f, column, name)?;
        f.write_str(message)
    }

    /// The diagnostic's place, before its message. A named file leads the
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
        Ok(())
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
    text: &'a str,
}

/// Each related place follows on a line of its own, as
/// `  --> lib.spit: line 9, column 5: step in the body of `summarise``.
impl fmt::Display for DisplayIn<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let diagnostic = self.diagnostic;
        if diagnostic.related.is_empty() {
            return diagnostic.write(f, self.column, self.name);
        }
        // The related places follow the message's first line, before the
        // lines that explain it.
        let (first, rest) = diagnostic
            .message
            .split_once('\n')
            .map_or((diagnostic.message.as_str(), None), |(first, rest)| {
                (first, Some(rest))
            });
        diagnostic.write_with(f, self.column, self.name, first)?;
        for related in &diagnostic.related {
            f.write_str("\n  --> ")?;
            let file = related.file.as_deref().or(self.name);
            if let Some(file) = file {
                write!(f, "{file}: ")?;
            }
            write!(f, "line {}", related.line)?;
            if let Some(column) = related
                .line_in(
                    self.diagnostic
                        .external_text
                        .as_deref()
                        .unwrap_or(self.text),
                )
                .and_then(|line| line.get(..related.columns.start))
            {
                write!(f, ", column {}", column.chars().count() + 1)?;
            }
            write!(f, ": {}", related.message)?;
        }
        if let Some(rest) = rest {
            write!(f, "\n{rest}")?;
        }
        Ok(())
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
        if !diagnostic.related.is_empty() {
            fields.push((
                "related",
                Json::array(diagnostic.related.iter().map(|related| {
                    let columns = related
                        .line_in(diagnostic.external_text.as_deref().unwrap_or(text))
                        .map(|line| utf16_columns(line, &related.columns));
                    let mut fields = Vec::with_capacity(6);
                    if let Some(file) = &related.file {
                        fields.push(("file", Json::string(file)));
                    }
                    fields.extend([
                        ("line", Json::number_or_null(Some(related.line))),
                        (
                            "column",
                            Json::number_or_null(columns.as_ref().map(|columns| columns.start + 1)),
                        ),
                        (
                            "end_column",
                            Json::number_or_null(columns.as_ref().map(|columns| columns.end + 1)),
                        ),
                        ("message", Json::string(&related.message)),
                    ]);
                    Json::object(fields)
                })),
            ));
        }
        Json::object(fields)
    });
    Json::array(items)
}
