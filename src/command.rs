//! Command templates, and static checks of commands against their operations.
//!
//! A command is stored as a list of arguments, each made of literal text and
//! `{placeholders}`. How the arguments are quoted for a shell, and where the
//! artifact paths are rooted, is left to a backend.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::model::{Cardinality, CommandRole, DefaultPort, OperationDef, Pipeline};
use crate::parser::SourceMap;
use crate::span::Located;
use crate::template::{parse_template, Part};

/// An error in a command, such as an unknown `{placeholder}`.
pub type CommandError = Located<String>;

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
    /// Split and parse a template such as `sort -o {output} {input}`,
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

pub(crate) fn slot(operation: &OperationDef, name: &str) -> Option<Slot> {
    if let Some(index) = operation.inputs.iter().position(|port| port.name == name) {
        return Some(Slot::Input(index));
    }
    if let Some(index) = operation.outputs.iter().position(|port| port.name == name) {
        return Some(Slot::Output(index));
    }
    match operation.inputs.as_slice() {
        [port] if port.cardinality == Cardinality::Many && name == DefaultPort::Inputs.name() => {
            Some(Slot::Input(0))
        }
        _ => None,
    }
}

/// Check every declared command against its operation without resolving jobs:
/// the template must name only known placeholders and write every output; a
/// `verify` command may read inputs only.
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

/// Check every command, collecting each error. Commands for operations in
/// `skip` belong to declarations that already failed and are not checked.
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
            match slot(operation, name).ok_or_else(unknown)? {
                Slot::Output(index) => {
                    if role == CommandRole::Verify {
                        return Err(CommandError::new(format!(
                            "verify for `{}` cannot use output `{{{name}}}`, which does not exist until the command runs",
                            operation.name
                        ))
                        .focus(format!("{{{name}}}")));
                    }
                    written.insert(index);
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
        if let Some(port) = operation
            .outputs
            .iter()
            .enumerate()
            .find(|(index, _)| !written.contains(index))
            .map(|(_, port)| port)
        {
            return Err(CommandError::new(format!(
                "command for `{}` must use `{{{}}}`",
                operation.name, port.name
            )));
        }
    }
    Ok(())
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
