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
                if !depends_on_invalid_operation(&error, &errors, &original_lines)
                    && !empties_failed_body(&error, &errors, &original_lines)
                {
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
    let declares = |line: &str| {
        line.trim()
            .strip_prefix("operation ")
            .is_some_and(|declaration| match declaration.split_once('(') {
                Some((name, _)) => name.trim() == operation,
                // Without `(` the name boundary is unknown, so accept a prefix.
                None => declaration.starts_with(operation.as_str()),
            })
    };
    previous_errors.iter().any(|previous| {
        let failed = previous.line().saturating_sub(1);
        if original_lines
            .get(failed)
            .is_some_and(|line| declares(line))
        {
            return true;
        }
        // An error in a step of the operation's body: the nearest line above
        // it that is not indented beneath a header declares the operation.
        original_lines
            .get(failed)
            .is_some_and(|line| line.starts_with(char::is_whitespace))
            && original_lines[..failed]
                .iter()
                .rev()
                .find(|line| !line.trim().is_empty() && !body_line(line))
                .is_some_and(|line| declares(line))
    })
}

/// Whether `line` is indented, as a step of an operation's body is.
fn body_line(line: &str) -> bool {
    line.starts_with(char::is_whitespace) && !line.trim_start().starts_with("operation ")
}

/// A body left with no steps because each of its steps already failed, and
/// was blanked, would only repeat those errors.
fn empties_failed_body(
    error: &ParseError,
    previous_errors: &[ParseError],
    original_lines: &[String],
) -> bool {
    if *error.kind() != ParseErrorKind::EmptyBody {
        return false;
    }
    let header = error.line();
    // The body's lines: those after the header, up to the first that is
    // not indented.
    let end = original_lines
        .iter()
        .enumerate()
        .skip(header)
        .find(|(_, line)| !line.trim().is_empty() && !line.starts_with(char::is_whitespace))
        .map_or(original_lines.len(), |(index, _)| index);
    previous_errors
        .iter()
        .any(|previous| previous.line() > header && previous.line() <= end)
}
