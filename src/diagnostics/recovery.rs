//! Reading on past a line that fails to parse, so that one bad line does not
//! hide the errors and editor information of the rest.

use crate::{ParseError, ParseErrorKind};

/// Blank only the line that failed, preserving all later line numbers. This
/// lets the existing parser continue to report errors on other lines without
/// changing the fail-fast parsing API used by the CLI and library callers.
pub(super) fn recover_parse_errors<T>(
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
