//! Command templates. [`validate`] checks them against their operations
//! and the steps that call them.
//!
//! A command is stored as a list of arguments, each made of literal text and
//! `{placeholders}`, and the ports its stdin and stdout are connected to,
//! written `< {input}` and `> {output}`. A backend runs the arguments as
//! they are; [`shell_word`] quotes one for a person to read or paste into a
//! shell.

mod validate;

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use crate::span::Located;
use crate::template::{parse_template, Part};

pub use self::validate::validate_commands;
pub(crate) use self::validate::{collect_commands, facet, slot, Facet, Slot};

/// An error in a command, such as an unknown `{placeholder}`.
pub type CommandError = Located<CommandProblem>;

crate::span::message_error!(
    /// What is wrong with a command template.
    CommandProblem
);

/// One argument of a command: literal text and `{placeholders}`, joined
/// without separators.
pub(crate) type Argument = Vec<Part>;

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
            Some([Part::Placeholder(name)]) => Some(name),
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
            let parse = |word: &Word| parse_template(&word.text).map_err(CommandError::new);
            let stream = match word.text.as_str() {
                "<" if word.bare => Some(Stream::Stdin),
                ">" if word.bare => Some(Stream::Stdout),
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
            if word.bare && is_shell_operator(&word.text) {
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
    /// The text, with quoted or escaped braces written `{{` and `}}`.
    text: String,
    /// Whether the word was written without quotes or backslashes.
    bare: bool,
    /// Where the word is in the template, quotes included.
    at: Range<usize>,
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
            (None, value) if value.is_whitespace() => {
                if started {
                    arguments.push(Word {
                        text: std::mem::take(&mut argument),
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
        arguments.push(Word {
            text: argument,
            bare,
            at: start..template.len(),
        });
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .map(|word| word.text.replace("{{", "{").replace("}}", "}"))
            .collect();
        assert_eq!(read, words);
    }
}
