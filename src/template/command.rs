use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use super::collection::{function_end, CollectionRead, JOIN_OPEN, LIST_OPEN};
use super::{builtin, parse_template, Expression, Part};
use crate::span::Located;

pub type CommandError = Located<CommandProblem>;

crate::span::message_error!(CommandProblem);

pub(crate) type Argument = Vec<CommandPart>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommandPart {
    Literal(String),
    Placeholder(String),
    Scalar(ScalarRead),
    Collection(CollectionRead),
}

pub(crate) const SCALAR: &str = "@scalar";
const SCALAR_OPEN: &str = "@scalar(";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScalarRead {
    pub(crate) port: String,
    pub(crate) min: Option<Arc<str>>,
    pub(crate) max: Option<Arc<str>>,
}

impl fmt::Display for ScalarRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&scalar_text(
            &self.port,
            self.min.as_deref(),
            self.max.as_deref(),
        ))
    }
}

pub(crate) fn scalar_text(subject: &str, min: Option<&str>, max: Option<&str>) -> String {
    let mut text = format!("{SCALAR}({subject}");
    for (key, bound) in [("min", min), ("max", max)] {
        if let Some(bound) = bound {
            text.push_str(&format!(", {key}={bound}"));
        }
    }
    text.push(')');
    text
}

impl ScalarRead {
    fn parse(inside: &str) -> Result<Self, String> {
        let written = format!("{SCALAR}({inside})");
        let mut items = inside.split(',').map(str::trim);
        let port = items.next().unwrap_or_default();
        let is_name = |text: &str| {
            !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        };
        if !is_name(port) {
            let bare = port.trim_start_matches('{').trim_end_matches('}');
            let hint = if is_name(bare) {
                format!("; write `{SCALAR}({bare})`, with no braces")
            } else {
                format!(", as in `{SCALAR}(tr, min=5)`")
            };
            return Err(format!("`{written}` must name an input port first{hint}"));
        }
        let mut read = Self {
            port: port.to_owned(),
            min: None,
            max: None,
        };
        for item in items {
            let Some((key, value)) = item.split_once('=') else {
                return Err(format!(
                    "`{written}`: `{item}` is not a bound; write `min=N` or `max=N`"
                ));
            };
            let bound = match key.trim() {
                "min" => &mut read.min,
                "max" => &mut read.max,
                other => {
                    return Err(format!(
                        "`{written}` has no `{other}`; its bounds are `min` and `max`"
                    ))
                }
            };
            if bound.is_some() {
                return Err(format!("`{written}` gives `{}` twice", key.trim()));
            }
            let value = value.trim();
            let number = decimal(value).ok_or_else(|| {
                format!(
                    "`{written}`: `{value}` is not a number; write a decimal such as `5` or `0.25`"
                )
            })?;
            *bound = Some(number.to_string().into());
        }
        if let (Some(min), Some(max)) = (&read.min, &read.max) {
            let parse = |text: &str| text.parse::<f64>().expect("a bound is a decimal");
            if parse(min) > parse(max) {
                return Err(format!(
                    "`{written}`: `min` is more than `max`, so no value passes"
                ));
            }
        }
        Ok(read)
    }
}

fn decimal(text: &str) -> Option<f64> {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match body.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (body, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |text: &str| text.chars().all(|c| c.is_ascii_digit());
    let exponent_ok = exponent.is_none_or(|exponent| {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });
    let ok = digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty());
    if !ok || !exponent_ok {
        return None;
    }
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandTemplate {
    text: String,
    calculation: Option<Arc<Expression>>,
    arguments: Vec<Argument>,
    operators: Vec<Range<usize>>,
    redirects: Vec<Redirect>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stream {
    Stdin,
    Stdout,
}

impl Stream {
    pub(crate) fn sign(self) -> &'static str {
        match self {
            Self::Stdin => "<",
            Self::Stdout => ">",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Redirect {
    pub(crate) stream: Stream,
    pub(crate) at: Range<usize>,
    pub(crate) target: Option<(Argument, Range<usize>)>,
}

impl Redirect {
    pub(crate) fn placeholder(&self) -> Option<&str> {
        match self.target.as_ref().map(|(parts, _)| parts.as_slice()) {
            Some([CommandPart::Placeholder(name)]) => Some(name),
            _ => None,
        }
    }
}

impl CommandTemplate {
    pub fn parse(text: impl Into<String>) -> Result<Self, CommandError> {
        let text = text.into();
        let words = split_arguments(&text)?;
        let mut operators = Vec::new();
        let mut redirects = Vec::new();
        let mut arguments = Vec::new();
        let mut words = words.into_iter();
        while let Some(word) = words.next() {
            let parse = |word: &Word| word.parse();
            let stream = match word.text() {
                Some("<") if word.bare => Some(Stream::Stdin),
                Some(">") if word.bare => Some(Stream::Stdout),
                _ => None,
            };
            if let Some(stream) = stream {
                let target = match words.next() {
                    Some(target) => Some((parse(&target)?, target.at)),
                    None => None,
                };
                redirects.push(Redirect {
                    stream,
                    at: word.at,
                    target,
                });
                continue;
            }
            if word.bare && word.text().is_some_and(is_shell_operator) {
                operators.push(word.at.clone());
            }
            arguments.push(parse(&word)?);
        }
        if arguments.is_empty() {
            return Err(CommandError::new(if redirects.is_empty() {
                "command template must not be empty"
            } else {
                "command template names no program to run, only where its stdin or stdout goes"
            }));
        }
        if let Some(CommandPart::Collection(read)) = arguments[0]
            .iter()
            .find(|part| matches!(part, CommandPart::Collection(_)))
        {
            return Err(CommandError::new(format!(
                "a command starts with the program it runs, and `{read}` gives a `many` input's files; put it after the program"
            ))
            .focus(read.to_string()));
        }
        if let Some(CommandPart::Scalar(read)) = arguments[0]
            .iter()
            .find(|part| matches!(part, CommandPart::Scalar(_)))
        {
            return Err(CommandError::new(format!(
                "a command starts with the program it runs, and `{read}` is a number; put it after the program"
            ))
            .focus(SCALAR));
        }
        Ok(Self {
            text,
            calculation: None,
            arguments,
            operators,
            redirects,
        })
    }

    pub(crate) fn calculation(text: &str, expression: Expression) -> Self {
        Self {
            text: text.to_owned(),
            calculation: Some(Arc::new(expression)),
            arguments: Vec::new(),
            operators: Vec::new(),
            redirects: Vec::new(),
        }
    }

    pub(crate) fn expression(&self) -> Option<&Arc<Expression>> {
        self.calculation.as_ref()
    }

    pub(crate) fn redirects(&self) -> &[Redirect] {
        &self.redirects
    }

    pub(crate) fn redirected(&self, stream: Stream) -> Option<&str> {
        self.redirects
            .iter()
            .find(|redirect| redirect.stream == stream)
            .and_then(Redirect::placeholder)
    }

    pub(crate) fn shell_operators(&self) -> &[Range<usize>] {
        &self.operators
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn arguments(&self) -> &[Argument] {
        &self.arguments
    }

    pub fn named_files(&self) -> impl Iterator<Item = NamedFile<'_>> {
        let relative = |text: &str| {
            text.contains('/')
                && !text.starts_with(['/', '-'])
                && !text.contains(char::is_whitespace)
        };
        self.arguments
            .iter()
            .filter_map(move |argument| match argument.as_slice() {
                [CommandPart::Literal(text)] if relative(text) => Some(NamedFile::Root(text)),
                [CommandPart::Placeholder(name), CommandPart::Literal(rest)]
                    if name == builtin::HERE && rest.starts_with('/') && relative(&rest[1..]) =>
                {
                    Some(NamedFile::Here(&rest[1..]))
                }
                _ => None,
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum NamedFile<'a> {
    Root(&'a str),
    Here(&'a str),
}

impl fmt::Display for NamedFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root(path) => f.write_str(path),
            Self::Here(path) => write!(f, "{{{}}}/{path}", builtin::HERE),
        }
    }
}

impl fmt::Display for CommandTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq<str> for CommandTemplate {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<&str> for CommandTemplate {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

pub(crate) fn shell_word(word: &str) -> Cow<'_, str> {
    let plain =
        |character: char| character.is_ascii_alphanumeric() || "_@%+=:,./-".contains(character);
    if !word.is_empty() && word.chars().all(plain) {
        return Cow::Borrowed(word);
    }
    Cow::Owned(format!("'{}'", word.replace('\'', "'\\''")))
}

struct Word {
    pieces: Vec<Piece>,
    bare: bool,
    at: Range<usize>,
}

enum Piece {
    Text(String),
    Scalar(String),
    Collection(String, bool),
}

impl Word {
    fn text(&self) -> Option<&str> {
        match self.pieces.as_slice() {
            [] => Some(""),
            [Piece::Text(text)] => Some(text),
            _ => None,
        }
    }

    fn parse(&self) -> Result<Argument, CommandError> {
        let mut parts: Argument = Vec::new();
        for piece in &self.pieces {
            match piece {
                Piece::Text(text) => {
                    for part in parse_template(text).map_err(CommandError::new)? {
                        match (part, parts.last_mut()) {
                            (Part::Literal(text), _) if text.is_empty() => {}
                            (Part::Literal(text), Some(CommandPart::Literal(last))) => {
                                last.push_str(&text);
                            }
                            (Part::Literal(text), _) => parts.push(CommandPart::Literal(text)),
                            (Part::Placeholder(name), _) => {
                                parts.push(CommandPart::Placeholder(name));
                            }
                        }
                    }
                }
                Piece::Collection(inside, list) => parts.push(CommandPart::Collection(
                    CollectionRead::parse(inside, *list)?,
                )),
                Piece::Scalar(inside) => {
                    parts.push(CommandPart::Scalar(ScalarRead::parse(inside).map_err(
                        |message| CommandError::new(message).focus(format!("{SCALAR}({inside})")),
                    )?))
                }
            }
        }
        if parts.is_empty() {
            parts.push(CommandPart::Literal(String::new()));
        }
        Ok(parts)
    }
}

fn is_shell_operator(word: &str) -> bool {
    if word.starts_with(['<', '>']) {
        return true;
    }
    let word = word.trim_start_matches(|character: char| character.is_ascii_digit());
    word.starts_with(['|', '&', ';', '<', '>'])
        && word
            .trim_end_matches(|character: char| character.is_ascii_digit() || character == '-')
            .chars()
            .all(|character| "|&;<>".contains(character))
}

fn split_arguments(template: &str) -> Result<Vec<Word>, CommandError> {
    let mut arguments = Vec::new();
    let mut pieces = Vec::new();
    let mut argument = String::new();
    let mut quote = None;
    let mut started = false;
    let mut bare = true;
    let mut start = 0;
    let literal = |argument: &mut String, value: char| match value {
        '{' => argument.push_str("{{"),
        '}' => argument.push_str("}}"),
        value => argument.push(value),
    };
    let mut chars = template.char_indices();
    while let Some((index, character)) = chars.next() {
        if !started && !(quote.is_none() && character.is_whitespace()) {
            start = index;
        }
        match (quote, character) {
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
                bare = false;
            }
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), value) => literal(&mut argument, value),
            (_, '\\') => {
                let (_, escaped) = chars
                    .next()
                    .ok_or_else(|| CommandError::new("trailing backslash in command"))?;
                if quote == Some('"') && !matches!(escaped, '"' | '\\' | '$' | '`') {
                    argument.push('\\');
                }
                literal(&mut argument, escaped);
                started = true;
                bare = false;
            }
            (None, '@')
                if template[index..].starts_with(JOIN_OPEN)
                    || template[index..].starts_with(LIST_OPEN) =>
            {
                let list = template[index..].starts_with(LIST_OPEN);
                let open = index + JOIN_OPEN.len();
                let end = function_end(template, open, &mut chars)?;
                if !argument.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut argument)));
                }
                pieces.push(Piece::Collection(template[open..end].to_owned(), list));
                started = true;
            }
            (None, '@') if template[index..].starts_with(SCALAR_OPEN) => {
                let open = index + SCALAR_OPEN.len();
                let close = template[open..].find(')').ok_or_else(|| {
                    CommandError::new(format!("`{SCALAR}(` is never closed with `)`")).focus(SCALAR)
                })?;
                if !argument.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut argument)));
                }
                pieces.push(Piece::Scalar(template[open..open + close].to_owned()));
                while chars.next().is_some_and(|(at, _)| at < open + close) {}
                started = true;
            }
            (None, value) if value.is_whitespace() => {
                if started {
                    if !argument.is_empty() || pieces.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut argument)));
                    }
                    arguments.push(Word {
                        pieces: std::mem::take(&mut pieces),
                        bare,
                        at: start..index,
                    });
                    started = false;
                    bare = true;
                }
            }
            (_, value) => {
                argument.push(value);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(CommandError::new("unterminated quote in command"));
    }
    if started {
        if !argument.is_empty() || pieces.is_empty() {
            pieces.push(Piece::Text(argument));
        }
        arguments.push(Word {
            pieces,
            bare,
            at: start..template.len(),
        });
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests {

    #[test]
    fn named_files_are_relative_paths_as_text() {
        let template = CommandTemplate::parse(
            "python3 ../../scripts/table.py --seconds {durations} /usr/bin/env -o out/{x} 'a b/c' scripts/run.sh {@here}/lib/fit.R {@here} > {@output}",
        )
        .unwrap();
        let files: Vec<_> = template.named_files().collect();
        assert_eq!(
            files,
            [
                NamedFile::Root("../../scripts/table.py"),
                NamedFile::Root("scripts/run.sh"),
                NamedFile::Here("lib/fit.R"),
            ]
        );
        assert_eq!(files[2].to_string(), "{@here}/lib/fit.R");
    }
    use super::*;

    #[test]
    fn a_decimal_is_a_finite_number_in_digits() {
        for (text, value) in [
            ("5", 5.0),
            ("-2", -2.0),
            ("+0.25", 0.25),
            (".5", 0.5),
            ("5.", 5.0),
            ("6E-3", 0.006),
        ] {
            assert_eq!(decimal(text), Some(value), "{text}");
        }
        for text in [
            "", ".", "-", "inf", "NaN", "1e", "1e+", "1_000", "0x10", "1e999", " 5",
        ] {
            assert_eq!(decimal(text), None, "{text}");
        }
    }

    #[test]
    fn a_plain_word_is_left_as_it_is() {
        for word in [
            "tool",
            "--out=build/x.csv",
            "a_b@c%d+e:f,g.h/i-j",
            "2026-09-01",
        ] {
            assert_eq!(shell_word(word), word);
        }
    }

    #[test]
    fn any_other_word_is_quoted() {
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("two words"), "'two words'");
        assert_eq!(shell_word("it's"), "'it'\\''s'");
        assert_eq!(shell_word("$HOME"), "'$HOME'");
        assert_eq!(shell_word("#note"), "'#note'");
        assert_eq!(shell_word("é"), "'é'");
    }

    #[test]
    fn quoted_words_split_back_into_the_same_words() {
        let words = [
            "tool",
            "",
            "two words",
            "it's",
            "'",
            "\\",
            "$x `y`",
            "*.csv",
            "~",
            "#c",
            "{a}",
            "a\"b",
            "tab\there",
            "é",
        ];
        let line: Vec<_> = words.iter().map(|word| shell_word(word)).collect();
        let split = split_arguments(&line.join(" ")).unwrap();
        let read: Vec<_> = split
            .iter()
            .map(|word| word.text().unwrap().replace("{{", "{").replace("}}", "}"))
            .collect();
        assert_eq!(read, words);
    }
}
