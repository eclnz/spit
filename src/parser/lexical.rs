//! Lexical helpers: names and comma-separated lists.

use super::keyword::reserved;
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
        .ok_or_else(|| ParseError::new(number, "expected `(` in operation call"))?;
    // From the `(` that is never closed to the end of the call.
    let opened = &line[name.len()..];
    let name = qualified_identifier(name.trim(), number, "operation name")?;
    let args = args
        .strip_suffix(')')
        .ok_or_else(|| ParseError::new(number, "expected closing `)`").at_token(opened))?;
    Ok((name, args))
}

pub(super) fn comma_items(text: &str, number: usize) -> Result<Vec<&str>, ParseError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    // The byte index of each opener not yet matched by a closer of its kind,
    // so an unexpected or unclosed bracket can point at the exact character.
    let mut brackets: Vec<usize> = Vec::new();
    let mut parens: Vec<usize> = Vec::new();
    let mut angles: Vec<usize> = Vec::new();
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '[' => brackets.push(index),
            ']' => {
                if brackets.pop().is_none() {
                    return Err(
                        ParseError::new(number, "unexpected `]`").at_token(&text[index..=index])
                    );
                }
            }
            '(' => parens.push(index),
            ')' => {
                if parens.pop().is_none() {
                    return Err(
                        ParseError::new(number, "unexpected `)`").at_token(&text[index..=index])
                    );
                }
            }
            '<' => angles.push(index),
            '>' => {
                if angles.pop().is_none() {
                    return Err(
                        ParseError::new(number, "unexpected `>`").at_token(&text[index..=index])
                    );
                }
            }
            ',' if parens.is_empty() && angles.is_empty() && brackets.is_empty() => {
                items.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if let Some((opener, &index)) = [
        ('(', parens.first()),
        ('<', angles.first()),
        ('[', brackets.first()),
    ]
    .into_iter()
    .find_map(|(opener, index)| index.map(|index| (opener, index)))
    {
        return Err(
            ParseError::new(number, format!("unclosed `{opener}`")).at_token(&text[index..=index])
        );
    }
    items.push(text[start..].trim());
    if items.iter().any(|item| item.is_empty()) {
        return Err(ParseError::new(
            number,
            "empty item in comma-separated list",
        ));
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
        Err(ParseError::new(
            number,
            format!("`{text}` is not an extension; write one such as `.mat` or `.nii.gz`, with letters, digits, `-` or `_` after each `.`"),
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
        return Err(ParseError::new(
            number,
            "a `+` suffix follows the whole name of the file it is written beside; add `beside` and that file's port or source, as in `-> Fai +.fai beside fasta`, or drop the `+`",
        )
        .at_token(text));
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
        return Err(ParseError::new(
            number,
            format!("invalid {kind} `{value}`; use letters, digits, and underscores"),
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
    ParseError::new(
        number,
        format!(
            "`{name}` is one of SPIT's keywords, so no product may take it as its name; rename it"
        ),
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
