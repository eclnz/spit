//! Lexical helpers: comments, names, and comma-separated lists.

use super::ParseError;

/// As in Bash, an unquoted `#` starts a comment only at the start of a word,
/// so arguments such as `--color=#fff` are kept intact.
pub(crate) fn strip_comment(line: &str) -> &str {
    &line[..comment_start(line).unwrap_or(line.len())]
}

fn comment_start(line: &str) -> Option<usize> {
    scan_hashes(line).find_map(|hash| hash.starts_word.then_some(hash.index))
}

/// The word before an unquoted `#` that ends it, as in `word# note`: the `#`
/// stays part of the word, though it reads like the start of a comment.
pub(crate) fn glued_comment(line: &str) -> Option<&str> {
    let end = comment_start(line).unwrap_or(line.len());
    scan_hashes(&line[..end]).find_map(|hash| {
        let ends_word = line[hash.index + 1..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace);
        (!hash.starts_word && ends_word).then(|| {
            let word_start = line[..hash.index]
                .rfind(char::is_whitespace)
                .map_or(0, |index| index + 1);
            &line[word_start..hash.index]
        })
    })
}

struct Hash {
    index: usize,
    starts_word: bool,
}

/// Every unquoted, unescaped `#` in `line`.
fn scan_hashes(line: &str) -> impl Iterator<Item = Hash> + '_ {
    let mut quote = None;
    let mut escaped = false;
    let mut word_start = true;
    line.char_indices().filter_map(move |(index, character)| {
        let at_word_start = word_start;
        word_start = false;
        if escaped {
            escaped = false;
            return None;
        }
        match (quote, character) {
            (None | Some('"'), '\\') => escaped = true,
            (None, '\'') => quote = Some('\''),
            (None, '"') => quote = Some('"'),
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (None, '#') => {
                return Some(Hash {
                    index,
                    starts_word: at_word_start,
                })
            }
            _ => {}
        }
        word_start = quote.is_none() && character.is_whitespace();
        None
    })
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
