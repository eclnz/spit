//! Command templates, and static checks of commands against their operations.
//!
//! A command is stored as a list of arguments, each made of literal text and
//! `{placeholders}`. A backend runs the arguments as they are; [`shell_word`]
//! quotes one for a person to read or paste into a shell.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::{Cardinality, CommandRole, OperationDef, Pipeline, DEFAULT_OUTPUT};
use crate::parser::SourceMap;
use crate::span::Located;
use crate::template::{parse_template, Part};

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
    /// Unquoted words a shell would read as operators, such as `|` or `>`.
    operators: Vec<String>,
}

impl CommandTemplate {
    /// Split and parse a template such as `sort -o {@output} {input}`,
    /// checking its quotes and braces. Arguments are separated by
    /// whitespace; quotes and backslashes keep text in one argument.
    pub fn parse(text: impl Into<String>) -> Result<Self, CommandError> {
        let text = text.into();
        let words = split_arguments(&text)?;
        let operators = words
            .iter()
            .filter(|word| word.bare && is_shell_operator(&word.text))
            .map(|word| word.text.clone())
            .collect();
        let arguments = words
            .iter()
            .map(|word| parse_template(&word.text).map_err(CommandError::new))
            .collect::<Result<Vec<_>, _>>()?;
        if arguments.is_empty() {
            return Err(CommandError::new("command template must not be empty"));
        }
        Ok(Self {
            text,
            arguments,
            operators,
        })
    }

    /// Unquoted words a shell would read as operators, such as `|` or `>`.
    pub fn shell_operators(&self) -> &[String] {
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

/// What a placeholder in a command refers to.
pub(crate) enum Slot {
    Input(usize),
    Output(usize),
}

/// What a placeholder gives of its port's file: the path, or, for an output
/// as `{image.dir}` and `{image.stem}`, its folder or its file name without
/// its extension, for a tool that takes a folder and a name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Facet {
    Path,
    Dir,
    Stem,
}

/// A placeholder's port and the facet of its file it gives, or the facet
/// written when it is not one.
pub(crate) fn facet(name: &str) -> (&str, Result<Facet, &str>) {
    match name.split_once('.') {
        None => (name, Ok(Facet::Path)),
        Some((port, "dir")) => (port, Ok(Facet::Dir)),
        Some((port, "stem")) => (port, Ok(Facet::Stem)),
        Some((port, other)) => (port, Err(other)),
    }
}

pub(crate) fn slot(operation: &OperationDef, name: &str) -> Option<Slot> {
    if name == "@output" {
        return operation
            .outputs
            .iter()
            .position(|port| port.name == DEFAULT_OUTPUT)
            .map(Slot::Output);
    }
    if name.starts_with('@') {
        return None;
    }
    if let Some(index) = operation.inputs.iter().position(|port| port.name == name) {
        return Some(Slot::Input(index));
    }
    operation
        .outputs
        .iter()
        .position(|port| port.name == name && port.name != DEFAULT_OUTPUT)
        .map(Slot::Output)
}

/// Check every declared command against its operation without resolving jobs:
/// the template must name only known placeholders and write every output; a
/// `verify` command may read inputs only. Checks are checked too.
pub fn validate_commands(pipeline: &Pipeline) -> Result<(), CommandError> {
    let lines = SourceMap::default();
    match collect_commands(pipeline, &lines, &BTreeSet::new())
        .into_iter()
        .next()
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Check every command, collecting each error, and then every `check` and
/// its uses. Commands for operations in `skip` belong to declarations that
/// already failed and are not checked.
pub(crate) fn collect_commands(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<CommandError> {
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, command) in pipeline.commands.iter().enumerate() {
        if skip.contains(&command.operation) {
            continue;
        }
        let line = lines.command(index);
        let Some(operation) = operations.get(command.operation.as_str()) else {
            errors.push(
                CommandError::new(format!(
                    "command refers to unknown operation `{}`",
                    command.operation
                ))
                .at(line)
                .focus(&command.operation),
            );
            continue;
        };
        if let Some(port) = operation.inputs.iter().find(|port| {
            operation
                .outputs
                .iter()
                .any(|output| output.name == port.name)
        }) {
            errors.push(
                CommandError::new(format!(
                    "operation `{}` has an input port named `{}`, which shadows `{{{}}}`",
                    operation.name, port.name, port.name
                ))
                .at(line.clone())
                .focus(&command.operation),
            );
        } else if command.role == CommandRole::Run && !seen.insert(command.operation.as_str()) {
            errors.push(
                CommandError::new(format!(
                    "duplicate command for operation `{}`",
                    command.operation
                ))
                .at(line)
                .focus(&command.operation),
            );
        } else if let Err(e) =
            check_command_placeholders(&command.template, operation, command.role)
        {
            errors.push(e.at(line));
        }
    }
    errors.extend(crate::check::collect_checks(pipeline, lines, skip));
    errors
}

fn check_command_placeholders(
    template: &CommandTemplate,
    operation: &OperationDef,
    role: CommandRole,
) -> Result<(), CommandError> {
    let mut written = BTreeSet::new();
    for parts in template.arguments() {
        let whole = parts.len() == 1;
        for part in parts {
            let Part::Placeholder(name) = part else {
                continue;
            };
            let unknown = || {
                CommandError::new(format!(
                    "command for `{}` uses unknown placeholder `{{{name}}}`",
                    operation.name
                ))
                .focus(format!("{{{name}}}"))
            };
            let fail =
                |message: String| Err(CommandError::new(message).focus(format!("{{{name}}}")));
            let (port, facet) = facet(name);
            let facet = match facet {
                Ok(facet) => facet,
                Err(other) => {
                    return fail(format!(
                        "`{{{name}}}` gives no `.{other}`; write `{{{port}}}` for the file, `{{{port}.dir}}` for its folder, or `{{{port}.stem}}` for its name without its extension"
                    ))
                }
            };
            let slot = slot(operation, port).ok_or_else(|| {
                if port == DEFAULT_OUTPUT
                    && operation
                        .outputs
                        .iter()
                        .any(|output| output.name == DEFAULT_OUTPUT)
                {
                    CommandError::new("`{output}` is now `{@output}` in commands")
                        .focus(format!("{{{name}}}"))
                } else {
                    unknown()
                }
            })?;
            match slot {
                Slot::Output(index) => {
                    if role == CommandRole::Verify {
                        return Err(CommandError::new(format!(
                            "verify for `{}` cannot use output `{{{name}}}`, which does not exist until the command runs",
                            operation.name
                        ))
                        .focus(format!("{{{name}}}")));
                    }
                    // A folder without an extension is its whole name.
                    let output = &operation.outputs[index];
                    if facet == Facet::Stem && output.extension.is_none() && !output.folder {
                        let example = if port == "@output" {
                            "-> Image .nii.gz".to_owned()
                        } else {
                            format!("{port}: Image .nii.gz")
                        };
                        let port_name = &operation.outputs[index].name;
                        return fail(format!(
                            "`{{{name}}}` is `{port_name}`'s file name without its extension, but `{}` declares none for `{port_name}`; give it one, as in `{example}`",
                            operation.name
                        ));
                    }
                    // A folder and a name tell the tool where to write.
                    written.insert(index);
                }
                Slot::Input(_) if facet != Facet::Path => {
                    return fail(format!(
                        "`{{{name}}}`: only an operation's outputs give `.dir` and `.stem`, for a tool that is told where to write"
                    ));
                }
                Slot::Input(index) => {
                    if operation.inputs[index].cardinality == Cardinality::Many && !whole {
                        return Err(CommandError::new(format!(
                            "many input `{{{name}}}` must be a complete command argument"
                        ))
                        .focus(format!("{{{name}}}")));
                    }
                }
            }
        }
    }
    if role == CommandRole::Run {
        // A tool writes an output beside another without being told where.
        if let Some(port) = operation
            .outputs
            .iter()
            .enumerate()
            .find(|(index, port)| port.beside.is_none() && !written.contains(index))
            .map(|(_, port)| port)
        {
            return Err(CommandError::new(format!(
                "command for `{}` must use `{{{}}}`",
                operation.name,
                if port.name == DEFAULT_OUTPUT {
                    "@output"
                } else {
                    &port.name
                }
            )));
        }
    }
    Ok(())
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
}

/// Whether `word` is a shell operator such as `|`, `&&`, `;`, `>`, or `2>&1`.
fn is_shell_operator(word: &str) -> bool {
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
    let literal = |argument: &mut String, value: char| match value {
        '{' => argument.push_str("{{"),
        '}' => argument.push_str("}}"),
        value => argument.push(value),
    };
    let mut chars = template.chars();
    while let Some(character) = chars.next() {
        match (quote, character) {
            (None, '\'' | '"') => {
                quote = Some(character);
                started = true;
                bare = false;
            }
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), value) => literal(&mut argument, value),
            (_, '\\') => {
                let escaped = chars
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
