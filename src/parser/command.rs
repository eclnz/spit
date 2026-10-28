//! `command` and `verify` lines, the shell operators every command
//! template, a check's included, is kept free of, and the `<` and `>` that
//! only an operation's command may use.

use crate::command::{CommandTemplate, Stream};
use crate::model::{CommandDef, CommandRole};

use super::declarations::single_colon;
use super::lexical::qualified_identifier;
use super::ParseError;

pub(super) fn parse_command(
    line: &str,
    number: usize,
    role: CommandRole,
) -> Result<CommandDef, ParseError> {
    let delimiter = declaration_delimiter(line).ok_or_else(|| {
        ParseError::new(
            number,
            match role {
                CommandRole::Run => "expected command: operation: executable [arguments]",
                CommandRole::Verify => "expected verify operation: executable [arguments]",
            },
        )
    })?;
    let (operation, rest) = line.split_at(delimiter);
    let operation = qualified_identifier(operation.trim(), number, "command operation")?;
    let template = rest[1..].trim();
    if template.is_empty() {
        return Err(ParseError::new(
            number,
            "command template must not be empty",
        ));
    }
    let parsed = CommandTemplate::parse(template).map_err(|error| {
        ParseError::new(
            number,
            format!("command `{operation}`: {}", error.message()),
        )
        .at_token(template)
    })?;
    let what = match role {
        CommandRole::Run => format!("the command for `{operation}`"),
        CommandRole::Verify => format!("the verify command for `{operation}`"),
    };
    reject_shell_operators(template, &parsed, &what, number)?;
    check_redirects(template, &parsed, &what, role == CommandRole::Run, number)?;
    Ok(CommandDef {
        role,
        ..CommandDef::new(operation, parsed)
    })
}

/// Reject an unquoted shell operator such as `>` or `|` in `template`, the
/// text `parsed` was read from. Commands run without a shell, so it would
/// reach the program as an argument and the job would fail when it runs.
pub(super) fn reject_shell_operators(
    template: &str,
    parsed: &CommandTemplate,
    what: &str,
    number: usize,
) -> Result<(), ParseError> {
    let Some(at) = parsed.shell_operators().first() else {
        return Ok(());
    };
    let operator = &template[at.clone()];
    let message = if operator.contains(['<', '>']) && !operator.contains(['|', '&', ';']) {
        format!(
            "`{operator}` in {what} is not read as a redirection, since commands run without a shell; quote it ('{operator}') to pass it to the program. A command's stdout is written to an output with `> {{@output}}`, and its stdin read from an input with `< {{input}}`, each sign a word of its own"
        )
    } else {
        format!(
            "`{operator}` in {what} is not read as a pipe or redirection, since commands run without a shell; quote it ('{operator}') to pass it to the program, or run a shell with the paths as its arguments, as in sh -c 'tool \"$1\" | sort > \"$2\"' sh {{input}} {{@output}}"
        )
    };
    Err(ParseError::new(number, message).at_token(operator))
}

/// Check each `<` and `>` in `template`, the text `parsed` was read from:
/// only an operation's command, where `allowed`, connects its stdin to an
/// input and its stdout to an output, once each, and the word after the
/// sign is one placeholder. Whether that names the right port is checked
/// against the operation.
pub(super) fn check_redirects(
    template: &str,
    parsed: &CommandTemplate,
    what: &str,
    allowed: bool,
    number: usize,
) -> Result<(), ParseError> {
    let mut seen = Vec::new();
    for redirect in parsed.redirects() {
        let sign = redirect.stream.sign();
        let (port, example) = match redirect.stream {
            Stream::Stdin => ("input", "< {input}"),
            Stream::Stdout => ("output", "> {@output}"),
        };
        let stream = match redirect.stream {
            Stream::Stdin => "stdin",
            Stream::Stdout => "stdout",
        };
        let fail = |message: String, at: &std::ops::Range<usize>| {
            Err(ParseError::new(number, message).at_token(&template[at.clone()]))
        };
        if !allowed {
            return fail(
                format!(
                    "`{sign}` in {what} is not read as a redirection: only an operation's `command` connects its {stream} to an {port}, as in `{example}`; quote it ('{sign}') to pass it to the program"
                ),
                &redirect.at,
            );
        }
        if seen.contains(&redirect.stream) {
            return fail(
                format!("{what} has a second `{sign}`; a command has one {stream}"),
                &redirect.at,
            );
        }
        seen.push(redirect.stream);
        match &redirect.target {
            None => {
                return fail(
                    format!(
                        "`{sign}` in {what} needs the {port} its {stream} goes to after it, as in `{example}`"
                    ),
                    &redirect.at,
                )
            }
            Some((_, at)) if redirect.placeholder().is_none() => {
                let target = &template[at.clone()];
                return fail(
                    format!(
                        "`{sign} {target}` in {what}: a command's {stream} is connected to one of its operation's {port}s, so write the {port}'s placeholder alone after `{sign}`, as in `{example}`"
                    ),
                    at,
                );
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn declaration_delimiter(line: &str) -> Option<usize> {
    [single_colon(line), line.find('=')]
        .into_iter()
        .flatten()
        .min()
}
