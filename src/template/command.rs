//! Command templates. `crate::command` checks them against their
//! operations and the steps that call them.
//!
//! A command is stored as a list of arguments, each made of literal text,
//! `{placeholders}` and `@scalar(...)` reads, and the ports its stdin and
//! stdout are connected to, written `< {input}` and `> {output}`. A backend
//! runs the arguments as they are; [`shell_word`] quotes one for a person to
//! read or paste into a shell.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use super::{builtin, parse_template, Part};
use crate::span::Located;

/// An error in a command, such as an unknown `{placeholder}`.
pub type CommandError = Located<CommandProblem>;

crate::span::message_error!(
    /// What is wrong with a command template.
    CommandProblem
);

/// One argument of a command: its parts, joined without separators.
pub(crate) type Argument = Vec<CommandPart>;

/// A piece of one argument of a command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommandPart {
    /// Text passed as it is.
    Literal(String),
    /// A `{name}` placeholder.
    Placeholder(String),
    /// A `@scalar(port, ...)`: the number in an input's file.
    Scalar(ScalarRead),
}

/// The word that starts a [`ScalarRead`], up to its `(`.
pub(crate) const SCALAR: &str = "@scalar";
const SCALAR_OPEN: &str = "@scalar(";

/// A `@scalar(port, min=N, max=N)` in a command: the number the file at an
/// input port holds, which a backend reads when the job starts and passes
/// as text, failing the job when it is not one finite number within the
/// bounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScalarRead {
    /// The input port whose file holds the number.
    pub(crate) port: String,
    /// The least value allowed, as the shortest decimal that reads back as
    /// it, which is also how JSON writes it. Shared, since each job's
    /// command holds it.
    pub(crate) min: Option<Arc<str>>,
    /// The greatest value allowed, written as `min` is.
    pub(crate) max: Option<Arc<str>>,
}

impl fmt::Display for ScalarRead {
    /// The read as a pipeline writes it, with its bounds in their shortest
    /// form: `@scalar(tr, min=5)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&scalar_text(
            &self.port,
            self.min.as_deref(),
            self.max.as_deref(),
        ))
    }
}

/// A `@scalar(...)` of `subject`, a port or a file's path, with its bounds:
/// `@scalar(tr, min=5)`.
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
    /// Read the text between `@scalar(` and `)`: a port, then `min=` and
    /// `max=` bounds, each at most once, separated by commas.
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

/// `text` as a finite number, when it is one written as a decimal: an
/// optional sign, digits with at most one `.`, and an optional exponent, as
/// `-2`, `0.25`, `.5` or `6e-3`. Words such as `inf` and `nan` are not.
/// Keep in step with `NUMBER` in the runner's `spit_bash/scalars.py`.
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

/// A command's template, split into arguments and parsed once when the
/// command is read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandTemplate {
    text: String,
    arguments: Vec<Argument>,
    /// Where in `text` each unquoted word a shell would read as an
    /// operator, such as `|` or `>>`, is, other than a redirection.
    operators: Vec<Range<usize>>,
    /// Each unquoted `<` or `>` word, with the word after it, in order.
    redirects: Vec<Redirect>,
}

/// Which of a command's streams a redirection connects to a file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stream {
    /// `< {input}`: the command reads an input on its stdin.
    Stdin,
    /// `> {output}`: the command's stdout is written to an output.
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

/// An unquoted `<` or `>` word of a command, and the word after it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Redirect {
    pub(crate) stream: Stream,
    /// Where the `<` or `>` is in the template's text.
    pub(crate) at: Range<usize>,
    /// The word after it, parsed, and where it is in the template's text;
    /// `None` at the end of the command.
    pub(crate) target: Option<(Argument, Range<usize>)>,
}

impl Redirect {
    /// The placeholder the redirection names, when the word after it is
    /// one placeholder and nothing else.
    pub(crate) fn placeholder(&self) -> Option<&str> {
        match self.target.as_ref().map(|(parts, _)| parts.as_slice()) {
            Some([CommandPart::Placeholder(name)]) => Some(name),
            _ => None,
        }
    }
}

impl CommandTemplate {
    /// Split and parse a template such as `sort -o {@output} {input}`,
    /// checking its quotes and braces. Arguments are separated by
    /// whitespace; quotes and backslashes keep text in one argument.
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
            arguments,
            operators,
            redirects,
        })
    }

    /// Each `<` or `>` word, with the word after it, in the order written.
    /// The parser allows them in an operation's command only, one of each,
    /// each followed by one placeholder.
    pub(crate) fn redirects(&self) -> &[Redirect] {
        &self.redirects
    }

    /// The placeholder the command's `stream` is redirected to, as in
    /// `> {@output}`, if any.
    pub(crate) fn redirected(&self, stream: Stream) -> Option<&str> {
        self.redirects
            .iter()
            .find(|redirect| redirect.stream == stream)
            .and_then(Redirect::placeholder)
    }

    /// Where in the template's text each unquoted word a shell would read
    /// as an operator, such as `|` or `>>`, is, other than a `<` or `>`
    /// redirection. The parser rejects them.
    pub(crate) fn shell_operators(&self) -> &[Range<usize>] {
        &self.operators
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn arguments(&self) -> &[Argument] {
        &self.arguments
    }

    /// Each file the command names as text rather than as a port: an
    /// argument that is literal text alone and reads as a relative path, as
    /// `../scripts/table.py`, or `{@here}` followed by one, as
    /// `{@here}/scripts/table.py`. A relative path holds a `/` and starts
    /// with neither `/` nor `-`; absolute paths are left out, since they name
    /// installed tools. A job doesn't declare such a file, so editing it
    /// reruns nothing. Whether a path names a file is for the caller, which
    /// knows the dataset root and the pipeline's folder, to find.
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

/// A file a command names as text, by where its path is relative to, as
/// [`CommandTemplate::named_files`] gives it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum NamedFile<'a> {
    /// Relative to the dataset root, where commands run.
    Root(&'a str),
    /// Relative to the folder of the file declaring the operation, written
    /// after `{@here}/`.
    Here(&'a str),
}

impl fmt::Display for NamedFile<'_> {
    /// The path as the command writes it.
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

/// `word` as a POSIX shell must be given it to read it back unchanged: as it
/// is when every character is one a shell leaves alone, and otherwise in
/// single quotes, with each `'` in it written `'\''`. The reverse of how a
/// template's words are read, so a line of quoted words splits back into
/// the words that were quoted.
pub(crate) fn shell_word(word: &str) -> Cow<'_, str> {
    let plain =
        |character: char| character.is_ascii_alphanumeric() || "_@%+=:,./-".contains(character);
    if !word.is_empty() && word.chars().all(plain) {
        return Cow::Borrowed(word);
    }
    Cow::Owned(format!("'{}'", word.replace('\'', "'\\''")))
}

/// A word of a command, before its placeholders are parsed.
struct Word {
    /// The word's text and its `@scalar(...)` reads, in order.
    pieces: Vec<Piece>,
    /// Whether the word was written without quotes or backslashes.
    bare: bool,
    /// Where the word is in the template, quotes included.
    at: Range<usize>,
}

/// A run of a [`Word`]'s text, or a `@scalar(...)` in it.
enum Piece {
    /// Text, with quoted or escaped braces written `{{` and `}}`.
    Text(String),
    /// What an unquoted `@scalar(` holds before its `)`.
    Scalar(String),
}

impl Word {
    /// The word's text, when it holds no `@scalar(...)`.
    fn text(&self) -> Option<&str> {
        match self.pieces.as_slice() {
            [] => Some(""),
            [Piece::Text(text)] => Some(text),
            _ => None,
        }
    }

    /// The word's parts, with its placeholders and reads parsed.
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

/// Whether `word` is a shell operator such as `|`, `&&`, `;`, `>`, or `2>&1`,
/// or a redirection written against its file, as in `>out.txt`.
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

/// Split a template into its words, removing quotes and backslashes. As in
/// Bash, single-quoted and backslash-escaped braces are literal.
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
                // As in Bash, a backslash inside double quotes escapes only
                // `"`, `\`, `$`, and `` ` ``; elsewhere it stays literal.
                if quote == Some('"') && !matches!(escaped, '"' | '\\' | '$' | '`') {
                    argument.push('\\');
                }
                literal(&mut argument, escaped);
                started = true;
                bare = false;
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
                // Move past the `)`.
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
        // Quoted braces read back doubled, as a template's literal braces do.
        let read: Vec<_> = split
            .iter()
            .map(|word| word.text().unwrap().replace("{{", "{").replace("}}", "}"))
            .collect();
        assert_eq!(read, words);
    }
}
