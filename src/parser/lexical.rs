//! Lexical helpers: names and comma-separated lists.

use super::error::LexicalError;
use super::keyword::reserved;
use super::scan::{self, Scanner};
use super::ParseError;

/// `text` without a UTF-8 byte order mark, which some Windows editors write.
/// Every public entry point that takes a document's text strips it, so the
/// rest of SPIT, and every column it reports, sees the text without one.
pub(crate) fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub(super) fn call_parts(line: &str, number: usize) -> Result<(&str, &str), ParseError> {
    let (name, args) = line
        .split_once('(')
        .ok_or_else(|| ParseError::of(number, LexicalError::ExpectedOpenParen))?;
    // From the `(` that is never closed to the end of the call.
    let opened = &line[name.len()..];
    let name = qualified_identifier(name.trim(), number, "operation name")?;
    let args = args
        .strip_suffix(')')
        .ok_or_else(|| ParseError::of(number, LexicalError::ExpectedCloseParen).at_token(opened))?;
    Ok((name, args))
}

pub(super) fn comma_items(text: &str, number: usize) -> Result<Vec<&str>, ParseError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut scanner = Scanner::new(text, scan::ALL);
    for mark in &mut scanner {
        if mark.stray {
            return Err(ParseError::of(
                number,
                LexicalError::UnexpectedCloser {
                    closer: mark.character.to_string(),
                },
            )
            .at_token(&text[mark.index..=mark.index]));
        }
        if mark.character == ',' && mark.depth == 0 {
            items.push(text[start..mark.index].trim());
            start = mark.index + 1;
        }
    }
    if let Some((opener, index)) = scanner.unclosed() {
        return Err(ParseError::of(
            number,
            LexicalError::UnclosedBracket {
                opener: opener.to_string(),
            },
        )
        .at_token(&text[index..=index]));
    }
    items.push(text[start..].trim());
    if items.iter().any(|item| item.is_empty()) {
        return Err(ParseError::of(number, LexicalError::EmptyListItem));
    }
    Ok(items)
}

/// A file extension such as `.mat` or `.nii.gz`: one or more parts, each a
/// `.` and letters, digits, `-` or `_`.
pub(super) fn extension(text: &str, number: usize) -> Result<&str, ParseError> {
    let valid = text.starts_with('.')
        && text[1..].split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
        });
    if valid {
        Ok(text)
    } else {
        Err(ParseError::of(
            number,
            LexicalError::NotAnExtension {
                text: text.to_owned(),
            },
        )
        .at_token(text))
    }
}

/// What ends a type in a declaration: an extension, a `/` that makes the
/// product a folder, or both, as in `Image .nii.gz`, `Dicom /` and
/// `Store .zarr/`. Gives the text before them, the extension, and whether
/// the product is a folder. Types hold no `.` or `/`, so the first `.`
/// starts the extension.
pub(super) fn split_ending(
    text: &str,
    number: usize,
) -> Result<(&str, Option<&str>, bool), ParseError> {
    let text = text.trim();
    let (text, folder) = match text.strip_suffix('/') {
        Some(rest) => (rest.trim_end(), true),
        None => (text, false),
    };
    let (head, extension) = match text.find('.') {
        Some(dot) => (
            text[..dot].trim(),
            Some(extension(text[dot..].trim(), number)?),
        ),
        None => (text, None),
    };
    if head.ends_with('+') {
        return Err(ParseError::of(number, LexicalError::PlusSuffixWithoutBeside).at_token(text));
    }
    Ok((head, extension, folder))
}

pub(super) fn identifier<'a>(
    value: &'a str,
    number: usize,
    kind: &str,
) -> Result<&'a str, ParseError> {
    let mut characters = value.chars();
    let valid_first = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    if !valid_first
        || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(ParseError::of(
            number,
            LexicalError::InvalidName {
                kind: kind.to_owned(),
                name: value.to_owned(),
            },
        )
        .at_token(value));
    }
    Ok(value)
}

/// A product's name, `value`: an identifier, and no keyword's word, which
/// is reserved.
pub(super) fn product_name<'a>(
    value: &'a str,
    number: usize,
    kind: &str,
) -> Result<&'a str, ParseError> {
    let name = identifier(value, number, kind)?;
    if reserved(name) {
        return Err(reserved_name(name, number));
    }
    Ok(name)
}

/// The error for a product named `name`, a keyword's reserved word.
pub(super) fn reserved_name(name: &str, number: usize) -> ParseError {
    ParseError::of(
        number,
        LexicalError::ReservedName {
            name: name.to_owned(),
        },
    )
    .at_token(name)
}

pub(super) fn qualified_identifier<'a>(
    value: &'a str,
    number: usize,
    kind: &str,
) -> Result<&'a str, ParseError> {
    for part in value.split("::") {
        identifier(part, number, kind)?;
    }
    Ok(value)
}
