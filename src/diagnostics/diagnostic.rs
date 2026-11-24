use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use crate::json::Json;
use crate::model::{ArtifactReport, SourceInventory};
use crate::parser::without_bom;
use crate::span::{utf16_columns, Lines, Located, Place};
use crate::Pipeline;

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
    pub columns: Option<Range<usize>>,
    pub message: String,
    pub code: String,
    pub file: Option<String>,
    pub external_text: Option<Arc<str>>,
    pub related: Vec<Related>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Related {
    pub file: Option<String>,
    pub line: usize,
    pub columns: Range<usize>,
    pub line_text: Option<String>,
    pub message: String,
}

impl Related {
    fn line_in<'a>(&'a self, lines: &Lines<'a>) -> Option<&'a str> {
        match &self.line_text {
            Some(line) => Some(line),
            None => lines.get(self.line),
        }
    }
}

pub struct SourceLines<'a> {
    pipeline: Lines<'a>,
    inventory: Option<Lines<'a>>,
    external: Vec<(&'a Arc<str>, Lines<'a>)>,
}

impl<'a> SourceLines<'a> {
    pub fn new(diagnostics: &'a [Diagnostic], text: &'a str, source_text: Option<&'a str>) -> Self {
        let mut external: Vec<(&'a Arc<str>, Lines<'a>)> = Vec::new();
        for diagnostic in diagnostics {
            let Some(other) = &diagnostic.external_text else {
                continue;
            };
            if !external.iter().any(|(known, _)| Arc::ptr_eq(known, other)) {
                external.push((other, Lines::new(without_bom(other))));
            }
        }
        Self {
            pipeline: Lines::new(without_bom(text)),
            inventory: source_text.map(|text| Lines::new(without_bom(text))),
            external,
        }
    }

    fn pipeline_of(&self, diagnostic: &Diagnostic) -> &Lines<'a> {
        let Some(other) = &diagnostic.external_text else {
            return &self.pipeline;
        };
        self.external
            .iter()
            .find(|(known, _)| Arc::ptr_eq(known, other))
            .map_or(&self.pipeline, |(_, lines)| lines)
    }
}

#[derive(Debug)]
pub struct Checked {
    pub pipeline: Pipeline,
    pub warnings: Vec<Diagnostic>,
    pub paths: Vec<ShownPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShownPath {
    pub product: String,
    pub line: usize,
    pub path: String,
}

#[derive(Debug)]
pub struct Records {
    pub inventory: SourceInventory,
    pub report: ArtifactReport,
}

pub type Diagnosis<T = Checked> = Result<T, Vec<Diagnostic>>;

pub(super) fn code_of(kind: &str, error: &dyn fmt::Debug) -> String {
    let debug = format!("{error:?}");
    let mut code = kind.to_owned();
    let mut rest = debug.as_str();
    loop {
        let end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let (name, after) = rest.split_at(end);
        if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_uppercase()) {
            break;
        }
        if let Some(inner) = after.strip_prefix(" { kind: ") {
            rest = inner;
            continue;
        }
        code.push('.');
        for (index, c) in name.chars().enumerate() {
            if c.is_ascii_uppercase() {
                if index > 0 {
                    code.push('-');
                }
                code.push(c.to_ascii_lowercase());
            } else {
                code.push(c);
            }
        }
        match after.strip_prefix('(') {
            Some(inner) => rest = inner,
            None => break,
        }
    }
    code
}

impl Diagnostic {
    pub(super) fn new(
        severity: Severity,
        source: DiagnosticSource,
        place: Option<Place>,
        code: String,
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
            code,
            file: None,
            external_text: None,
            related: Vec::new(),
        }
    }

    pub(super) fn located<E: fmt::Display + fmt::Debug>(
        kind: &str,
        source: DiagnosticSource,
        error: &Located<E>,
        text: &str,
    ) -> Self {
        let code = code_of(kind, &error.error);
        if let Some(imported) = error.location.imported() {
            return Self {
                severity: Severity::Error,
                source,
                line: Some(imported.place.line),
                columns: Some(imported.place.columns.clone()),
                message: error.message(),
                code,
                file: Some(imported.file.clone()),
                external_text: Some(Arc::clone(&imported.text)),
                related: imported
                    .uses
                    .iter()
                    .map(|used| Related {
                        file: used.file.clone(),
                        line: used.place.line,
                        columns: used.place.columns.clone(),
                        line_text: Some(used.text.clone()),
                        message: "imported here".to_owned(),
                    })
                    .collect(),
            };
        }
        let place = error.location.place_in(text);
        Self {
            severity: Severity::Error,
            source,
            line: error.location.line,
            columns: place.map(|place| place.columns),
            message: error.message(),
            code,
            file: None,
            external_text: None,
            related: Vec::new(),
        }
    }

    pub(super) fn error(
        source: DiagnosticSource,
        place: Option<Place>,
        code: String,
        message: String,
    ) -> Self {
        Self::new(Severity::Error, source, place, code, message)
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn utf16_columns(&self, text: &str, source_text: Option<&str>) -> Option<Range<usize>> {
        let diagnostics = std::slice::from_ref(self);
        self.utf16_columns_in(&SourceLines::new(diagnostics, text, source_text))
    }

    pub fn utf16_columns_in(&self, lines: &SourceLines<'_>) -> Option<Range<usize>> {
        let columns = self.columns.as_ref()?;
        Some(utf16_columns(self.line_text(lines)?, columns))
    }

    pub fn display_in<'a>(
        &'a self,
        text: &'a str,
        source_text: Option<&'a str>,
    ) -> impl fmt::Display + 'a {
        self.display_named(text, source_text, FileNames::default())
    }

    pub fn display_named<'a>(
        &'a self,
        text: &'a str,
        source_text: Option<&'a str>,
        names: FileNames<'a>,
    ) -> impl fmt::Display + 'a {
        let diagnostics = std::slice::from_ref(self);
        let lines = SourceLines::new(diagnostics, text, source_text);
        self.display_with(&lines, names)
    }

    pub fn display_with<'a>(
        &'a self,
        lines: &SourceLines<'a>,
        names: FileNames<'a>,
    ) -> impl fmt::Display + 'a {
        DisplayIn {
            diagnostic: self,
            column: self.column_in(lines),
            name: self.name_in(names),
            related_columns: self
                .related
                .iter()
                .map(|related| {
                    related
                        .line_in(lines.pipeline_of(self))
                        .and_then(|line| line.get(..related.columns.start))
                        .map(|before| before.chars().count() + 1)
                })
                .collect(),
        }
    }

    fn column_in(&self, lines: &SourceLines<'_>) -> Option<usize> {
        let columns = self.columns.as_ref()?;
        let line = self.line_text(lines)?;
        Some(line.get(..columns.start)?.chars().count() + 1)
    }

    fn name_in<'a>(&'a self, names: FileNames<'a>) -> Option<&'a str> {
        if let Some(file) = &self.file {
            return Some(file);
        }
        match self.source {
            DiagnosticSource::Pipeline => names.pipeline,
            DiagnosticSource::Inventory => names.inventory,
        }
    }

    fn line_text<'a>(&self, lines: &SourceLines<'a>) -> Option<&'a str> {
        let lines = match self.source {
            DiagnosticSource::Inventory => lines.inventory.as_ref()?,
            DiagnosticSource::Pipeline => lines.pipeline_of(self),
        };
        lines.get(self.line?)
    }

    fn write(
        &self,
        f: &mut fmt::Formatter<'_>,
        column: Option<usize>,
        name: Option<&str>,
    ) -> fmt::Result {
        self.write_with(f, column, name, &self.message)
    }

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

#[derive(Clone, Copy, Debug, Default)]
pub struct FileNames<'a> {
    pub pipeline: Option<&'a str>,
    pub inventory: Option<&'a str>,
}

struct DisplayIn<'a> {
    diagnostic: &'a Diagnostic,
    column: Option<usize>,
    name: Option<&'a str>,
    related_columns: Vec<Option<usize>>,
}

impl fmt::Display for DisplayIn<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let diagnostic = self.diagnostic;
        if diagnostic.related.is_empty() {
            return diagnostic.write(f, self.column, self.name);
        }
        let (first, rest) = diagnostic
            .message
            .split_once('\n')
            .map_or((diagnostic.message.as_str(), None), |(first, rest)| {
                (first, Some(rest))
            });
        diagnostic.write_with(f, self.column, self.name, first)?;
        for (related, column) in diagnostic.related.iter().zip(&self.related_columns) {
            f.write_str("\n  --> ")?;
            let file = related.file.as_deref().or(self.name);
            if let Some(file) = file {
                write!(f, "{file}: ")?;
            }
            write!(f, "line {}", related.line)?;
            if let Some(column) = column {
                write!(f, ", column {column}")?;
            }
            write!(f, ": {}", related.message)?;
        }
        if let Some(rest) = rest {
            write!(f, "\n{rest}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, None, None)
    }
}

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

pub fn render_check_json(diagnostics: &[Diagnostic], text: &str, paths: &[ShownPath]) -> String {
    format!(
        "{}\n",
        Json::object([
            ("diagnostics", diagnostics_json(diagnostics, text, None)),
            ("paths", shown_paths_json(paths)),
        ])
    )
}

pub fn render_calls_check_json(diagnostics: &[Diagnostic], text: &str, calls: &str) -> String {
    let mut out = String::new();
    let mut object = crate::json::ObjectWriter::start(&mut out);
    object.field("diagnostics", |out| {
        diagnostics_json(diagnostics, text, None).write_to(out);
    });
    object.raw("calls", calls);
    object.finish();
    out.push('\n');
    out
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
    text: &'a str,
    source_text: Option<&'a str>,
) -> Json<'a> {
    let lines = SourceLines::new(diagnostics, text, source_text);
    let items = diagnostics.iter().map(|diagnostic| {
        let columns = diagnostic.utf16_columns_in(&lines);
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
            ("code", Json::string(&diagnostic.code)),
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
                        .line_in(lines.pipeline_of(diagnostic))
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

#[cfg(test)]
mod tests {
    use super::code_of;

    #[derive(Debug)]
    #[allow(dead_code)]
    enum Inner {
        ExpectedColonAfterName,
        Holds { name: String },
    }

    #[derive(Debug)]
    #[allow(dead_code)]
    enum Outer {
        Declaration(Inner),
        Plain,
    }

    #[derive(Debug)]
    #[allow(dead_code)]
    struct Wrapper {
        kind: Box<Outer>,
    }

    #[test]
    fn a_code_is_the_variant_names_in_lower_case() {
        assert_eq!(code_of("x", &Outer::Plain), "x.plain");
        assert_eq!(
            code_of("parse", &Outer::Declaration(Inner::ExpectedColonAfterName)),
            "parse.declaration.expected-colon-after-name"
        );
        assert_eq!(
            code_of(
                "parse",
                &Outer::Declaration(Inner::Holds {
                    name: "a".to_owned()
                })
            ),
            "parse.declaration.holds"
        );
        let wrapped = Wrapper {
            kind: Box::new(Outer::Plain),
        };
        assert_eq!(code_of("parse", &wrapped), "parse.plain");
    }
}
