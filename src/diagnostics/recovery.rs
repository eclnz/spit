//! Reading on past a line that fails to parse, so that one bad line does not
//! hide the errors and editor information of the rest.

use std::collections::BTreeSet;

use rustc_hash::FxHashSet;

use crate::parser::{parse_use, strip_comment, Keyword, UseSpec};
use crate::{ParseError, ParseErrorKind};

/// Blank the lines that failed, preserving all later line numbers. This
/// lets the existing parser continue to report errors on other lines without
/// changing the fail-fast parsing API used by the CLI and library callers.
pub(super) fn recover_parse_errors<T>(
    text: &str,
    parse: impl Fn(&str) -> Result<T, Vec<ParseError>>,
) -> Result<T, Vec<ParseError>> {
    let (parsed, errors) = recover_document(text, parse);
    match parsed {
        Some(parsed) if errors.is_empty() => Ok(parsed),
        _ => Err(errors),
    }
}

/// Keep the independently parseable declarations for editor information,
/// while retaining every error for callers that require a valid document.
///
/// Each parse gives the errors that blanking the line of the first, and then
/// of the next, would find one after the other, so that its time is not paid
/// again for each of them. They are taken in order, each as if the lines
/// before it were already blank.
pub(crate) fn recover_document<T>(
    text: &str,
    parse: impl Fn(&str) -> Result<T, Vec<ParseError>>,
) -> (Option<T>, Vec<ParseError>) {
    let original_lines: Vec<&str> = text.lines().collect();
    // Keep in step with operation_lines in src/parser/continuation.rs: a
    // signature is one declaration even when its error names a port line.
    let mut signatures = vec![None; original_lines.len()];
    for (start, line) in crate::parser::operation_lines(text) {
        let end = start + line.lines().count();
        if end > start + 1 {
            signatures[start..end].fill(Some((start, end)));
        }
    }
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
    let mut failures = Failures::new(&original_lines);
    loop {
        let found = match parse(&recovered) {
            Ok(parsed) => return (Some(parsed), errors),
            Err(found) => found,
        };
        for error in found {
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
            if let Some((start, end)) = signatures[error.line() - 1] {
                for range in &ranges[start..end] {
                    recovered.replace_range(range.clone(), &" ".repeat(range.len()));
                }
            }
            // Misplaced records are one error, however many lines.
            if let ParseErrorKind::MisplacedRecords { lines: records } = error.kind() {
                for record in records {
                    if let Some(range) = ranges.get(record - 1) {
                        recovered.replace_range(range.clone(), &" ".repeat(range.len()));
                    }
                }
            }
            if !failures.explains(&error) && !failures.empties_body(&error) {
                failures.note(&error);
                errors.push(error);
            }
        }
    }
}

/// What the errors kept so far say of the operations, so that an error that
/// only repeats one is left out. Each question is answered from tables
/// built as the errors come, not by reading every earlier error again.
struct Failures<'a> {
    lines: &'a [&'a str],
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
    /// The line of each error kept.
    error_lines: BTreeSet<usize>,
}

impl<'a> Failures<'a> {
    fn new(lines: &'a [&'a str]) -> Self {
        let mut above = Vec::with_capacity(lines.len());
        let mut header = None;
        for (index, line) in lines.iter().enumerate() {
            above.push(header);
            if !line.trim().is_empty() && !body_line(line) {
                header = Some(index);
            }
        }
        Self {
            lines,
            above,
            named: FxHashSet::default(),
            unnamed: Vec::new(),
            failed_uses: Vec::new(),
            error_lines: BTreeSet::new(),
        }
    }

    /// Keep `error`, which is reported.
    fn note(&mut self, error: &ParseError) {
        self.error_lines.insert(error.line());
        let failed = error.line().saturating_sub(1);
        let Some(&line) = self.lines.get(failed) else {
            return;
        };
        self.declare(line);
        // An error in a step of the operation's body: the nearest line above
        // it that is not indented beneath a header declares the operation.
        if line.starts_with(char::is_whitespace) {
            if let Some(header) = self.above[failed] {
                self.declare(self.lines[header]);
            }
        }
    }

    /// Note the operation `line` declares, if it declares one.
    fn declare(&mut self, line: &'a str) {
        // A `use` line that failed brought in no operation, so a call of one
        // it would have brought in only repeats its error.
        let trimmed = strip_comment(line).trim();
        if Keyword::of(trimmed) == Some(Keyword::Use) {
            if let Ok(spec) = parse_use(trimmed, 0) {
                self.failed_uses.push(spec);
            }
            return;
        }
        let Some(declaration) = line.trim().strip_prefix("operation ") else {
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

    /// A body left with no steps because each of its steps already failed,
    /// and was blanked, would only repeat those errors.
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
            .find(|(_, line)| !line.trim().is_empty() && !line.starts_with(char::is_whitespace))
            .map_or(self.lines.len(), |(index, _)| index);
        header < end && self.error_lines.range(header + 1..=end).next().is_some()
    }
}

/// Whether `line` is indented, as a step of an operation's body is.
fn body_line(line: &str) -> bool {
    line.starts_with(char::is_whitespace) && !line.trim_start().starts_with("operation ")
}
