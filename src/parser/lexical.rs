use super::error::LexicalError;
use super::keyword::reserved;
use super::scan::{self, Scanner};
use super::ParseError;

pub(crate) fn without_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub(super) fn call_parts(line: &str, number: usize) -> Result<(&str, &str), ParseError> {
    let (name, args) = line
        .split_once('(')
        .ok_or_else(|| ParseError::of(number, LexicalError::ExpectedOpenParen))?;
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
                    closer: mark.character,
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
        return Err(
            ParseError::of(number, LexicalError::UnclosedBracket { opener })
                .at_token(&text[index..=index]),
        );
    }
    items.push(text[start..].trim());
    if items.iter().any(|item| item.is_empty()) {
        return Err(ParseError::of(number, LexicalError::EmptyListItem));
    }
    Ok(items)
}

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
    kind: &'static str,
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
                kind,
                name: value.to_owned(),
            },
        )
        .at_token(value));
    }
    Ok(value)
}

pub(super) fn product_name<'a>(
    value: &'a str,
    number: usize,
    kind: &'static str,
) -> Result<&'a str, ParseError> {
    let name = identifier(value, number, kind)?;
    if reserved(name) {
        return Err(reserved_name(name, number));
    }
    Ok(name)
}

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
    kind: &'static str,
) -> Result<&'a str, ParseError> {
    for part in value.split("::") {
        identifier(part, number, kind)?;
    }
    Ok(value)
}
