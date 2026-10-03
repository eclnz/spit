//! `command` and `verify` lines, and the shell operators every command
//! template, a check's included, is kept free of.

use crate::command::CommandTemplate;
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
    reject_shell_operators(
        template,
        &parsed,
        &format!("the command for `{operation}`"),
        number,
    )?;
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
    Err(ParseError::new(
        number,
        format!(
            "`{operator}` in {what} is not read as a pipe or redirection, since commands run without a shell; quote it ('{operator}') to pass it to the program, or run a shell with the paths as its arguments, as in sh -c 'tool \"$1\" | sort > \"$2\"' sh {{input}} {{@output}}"
        ),
    )
    .at_token(operator))
}

fn declaration_delimiter(line: &str) -> Option<usize> {
    [single_colon(line), line.find('=')]
        .into_iter()
        .flatten()
        .min()
}
