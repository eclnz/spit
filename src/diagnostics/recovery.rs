//! Reading on past a line that fails to parse, so that one bad line does not
//! hide the errors and editor information of the rest.

use std::collections::BTreeSet;

use rustc_hash::FxHashSet;

use crate::parser::{parse_use, Keyword, Layout, Line, UseSpec};
use crate::{ParseError, ParseErrorKind};

/// The document `parse` reads from `text`, or every error it has.
pub(super) fn recover_parse_errors<T>(
    text: &str,
    parse: impl FnOnce(&str) -> (T, Vec<ParseError>),
) -> Result<T, Vec<ParseError>> {
    let (parsed, errors) = recover_document(text, parse);
    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(errors)
    }
}

/// The document `parse` reads from `text`, with the statements that failed
/// left out, and its errors less those that only repeat another.
///
/// The parse reads past a statement that fails, leaving out all of it, so
/// one parse finds every error. An error that follows from another, such as
/// a call to an operation whose declaration failed, is left out.
pub(crate) fn recover_document<T>(
    text: &str,
    parse: impl FnOnce(&str) -> (T, Vec<ParseError>),
) -> (T, Vec<ParseError>) {
    let (parsed, mut found) = parse(text);
    if found.is_empty() {
        return (parsed, found);
    }
    // An error is explained by one on an earlier line.
    found.sort_by_key(ParseError::line);
    let layout = Layout::new(text);
    let mut failures = Failures::new(&layout.lines, found.iter().map(ParseError::line).collect());
    let mut errors = Vec::new();
    for error in found {
        if !failures.explains(&error) && !failures.empties_body(&error) {
            failures.note(&error);
            errors.push(error);
        }
    }
    (parsed, errors)
}

/// What the errors kept so far say of the operations, so that an error that
/// only repeats one is left out. Each question is answered from tables
/// built as the errors come, not by reading every earlier error again.
struct Failures<'a> {
    lines: &'a [Line<'a>],
    /// For each line, the nearest line above it that is not blank and not a
    /// step of a body: the header of the body the line is in.
    above: Vec<Option<usize>>,
    /// The names of the operations whose declarations an error is in or
    /// beneath, where the declaration gives one before its `(`.
    named: FxHashSet<String>,
    /// The declarations with no `(`, where the name boundary is unknown, so
    /// a call's name matches as a prefix.
    unnamed: Vec<&'a str>,
    /// The `use` lines that failed.
    failed_uses: Vec<UseSpec>,
    /// The line of each error found.
    error_lines: BTreeSet<usize>,
}

impl<'a> Failures<'a> {
    fn new(lines: &'a [Line<'a>], error_lines: BTreeSet<usize>) -> Self {
        let mut above = Vec::with_capacity(lines.len());
        let mut header = None;
        for (index, line) in lines.iter().enumerate() {
            above.push(header);
            if !line.text.trim().is_empty() && !body_line(line.text) {
                header = Some(index);
            }
        }
        Self {
            lines,
            above,
            named: FxHashSet::default(),
            unnamed: Vec::new(),
            failed_uses: Vec::new(),
            error_lines,
        }
    }

    /// Keep `error`, which is reported.
    fn note(&mut self, error: &ParseError) {
        let failed = error.line().saturating_sub(1);
        let Some(line) = self.lines.get(failed) else {
            return;
        };
        self.declare(line);
        // An error in a step of the operation's body: the nearest line above
        // it that is not indented beneath a header declares the operation.
        if line.text.starts_with(char::is_whitespace) {
            if let Some(header) = self.above[failed] {
                self.declare(&self.lines[header]);
            }
        }
    }

    /// Note the operation `line` declares, if it declares one.
    fn declare(&mut self, line: &Line<'a>) {
        // A `use` line that failed brought in no operation, so a call of one
        // it would have brought in only repeats its error.
        if line.keyword() == Some(Keyword::Use) {
            if let Ok(spec) = parse_use(line.code, 0) {
                self.failed_uses.push(spec);
            }
            return;
        }
        let Some(declaration) = line.text.trim().strip_prefix("operation ") else {
            return;
        };
        match declaration.split_once('(') {
            Some((name, _)) => {
                self.named.insert(name.trim().to_owned());
            }
            None => self.unnamed.push(declaration),
        }
    }

    /// A call to an operation whose own declaration already failed to parse
    /// would only repeat that error, so it is not reported separately.
    fn explains(&self, error: &ParseError) -> bool {
        let ParseErrorKind::UndeclaredOperation { name: operation } = error.kind() else {
            return false;
        };
        self.named.contains(operation.as_str())
            || self.failed_uses.iter().any(|spec| {
                let local = match &spec.alias {
                    Some(alias) => operation
                        .strip_prefix(alias.as_str())
                        .and_then(|rest| rest.strip_prefix("::")),
                    None => Some(operation.as_str()),
                };
                local.is_some_and(|local| {
                    spec.names
                        .as_ref()
                        .is_none_or(|names| names.iter().any(|name| name == local))
                })
            })
            || self
                .unnamed
                .iter()
                .any(|declaration| declaration.starts_with(operation.as_str()))
    }

    /// A body left with no steps because each of its steps already failed
    /// would only repeat those errors.
    fn empties_body(&self, error: &ParseError) -> bool {
        if *error.kind() != ParseErrorKind::EmptyBody {
            return false;
        }
        let header = error.line();
        // The body's lines: those after the header, up to the first that is
        // not indented.
        let end = self
            .lines
            .iter()
            .enumerate()
            .skip(header)
            .find(|(_, line)| {
                !line.text.trim().is_empty() && !line.text.starts_with(char::is_whitespace)
            })
            .map_or(self.lines.len(), |(index, _)| index);
        header < end && self.error_lines.range(header + 1..=end).next().is_some()
    }
}

/// Whether `line` is indented, as a step of an operation's body is.
fn body_line(line: &str) -> bool {
    line.starts_with(char::is_whitespace) && !line.trim_start().starts_with("operation ")
}
