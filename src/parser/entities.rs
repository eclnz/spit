//! Pipeline-wide entity assignment templates and dimension label aliases.

use crate::template::EntitiesFormat;

use super::error::EntityError;
use super::lexical::identifier;
use super::ParseError;

#[derive(Clone, Debug)]
pub(crate) enum EntitiesDeclaration {
    Format(EntitiesFormat),
    Label { dimension: String, label: String },
}

/// Read a `default entities` setting: `entities: {@key}={@value}
/// separated "__"`, the spelling, or `default entities sub: subject`, a label.
pub(super) fn parse_entities(line: &str, number: usize) -> Result<EntitiesDeclaration, ParseError> {
    if let Some(rest) = line.strip_prefix("entities:") {
        let (template, tail) = rest
            .trim()
            .split_once(" separated ")
            .ok_or_else(|| ParseError::of(number, EntityError::ExpectedDefaultEntitiesSeparated))?;
        let template = unquote(template.trim());
        let (separator, rest) = quoted(tail.trim(), number)?;
        if !rest.trim().is_empty() {
            return Err(ParseError::of(
                number,
                EntityError::UnexpectedTextAfterEntitiesSeparatorProduct,
            )
            .at_token(rest.trim()));
        }
        let format = EntitiesFormat::parse(template, separator).map_err(|message| {
            ParseError::of(number, EntityError::Format { message }).at_token(template)
        })?;
        return Ok(EntitiesDeclaration::Format(format));
    }
    let rest = line.strip_prefix("entities ").unwrap_or_default();
    let (dimension, label) = rest.split_once(':').ok_or_else(|| {
        ParseError::of(number, EntityError::ExpectedDefaultEntitiesDimensionLabel)
    })?;
    let dimension = identifier(dimension.trim(), number, "entity dimension")?;
    let label = identifier(label.trim(), number, "entity label")?;
    Ok(EntitiesDeclaration::Label {
        dimension: dimension.to_owned(),
        label: label.to_owned(),
    })
}

fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| text.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(text)
}

fn quoted(text: &str, number: usize) -> Result<(&str, &str), ParseError> {
    let quote = text
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\''))
        .ok_or_else(|| {
            ParseError::of(number, EntityError::EntitiesSeparatorMustQuoted).at_token(text)
        })?;
    let end = text[1..].find(quote).ok_or_else(|| {
        ParseError::of(number, EntityError::UnclosedQuotedEntitiesText).at_token(text)
    })? + 1;
    let tail = &text[end + 1..];
    if !tail.is_empty() && !tail.starts_with(char::is_whitespace) {
        return Err(ParseError::of(
            number,
            EntityError::ExpectedWhitespaceAfterQuotedEntitiesText,
        )
        .at_token(tail));
    }
    Ok((&text[1..end], tail))
}
