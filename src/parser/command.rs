//! `command` and `verify` lines, a `command` block's lines run in order,
//! the shell operators every command template, a check's included, is kept
//! free of, and the `<` and `>` that only an operation's command may use.

use crate::command::{CommandTemplate, Stream};
use crate::model::{CommandDef, CommandRole};
use crate::span::{content_columns, Place};

use super::declarations::single_colon;
use super::lexical::qualified_identifier;
use super::source_map::{tail_place, CommandPlaces};
use super::ParseError;

pub(super) fn parse_command(
    original: &str,
    line: &str,
    number: usize,
    role: CommandRole,
) -> Result<(CommandDef, CommandPlaces), ParseError> {
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
    let rest = &rest[1..];
    let what = match role {
        CommandRole::Run => format!("the command for `{operation}`"),
        CommandRole::Verify => format!("the verify command for `{operation}`"),
    };
    // Keep in step with `parse_check` in `src/parser/check.rs`, which
    // rejects a check written this way.
    let first = rest.split('\n').next().unwrap_or_default();
    let (texts, places) = if first.trim().is_empty() && rest.contains('\n') {
        if role == CommandRole::Verify {
            return Err(ParseError::new(
                number,
                format!(
                    "a `verify` line holds one command, written after its `:`; give each command a `verify {operation}: ...` line of its own"
                ),
            )
            .at_token(operation));
        }
        let header = original.split('\n').next().unwrap_or_default();
        let (texts, lines) = sequence_lines(rest, number, operation)?;
        let header = Place::new(number, content_columns(header));
        (texts, CommandPlaces { header, lines })
    } else {
        let template = rest.trim();
        let place = tail_place(original, number, template);
        (
            vec![template],
            CommandPlaces {
                header: place.clone(),
                lines: vec![place],
            },
        )
    };
    if texts.is_empty() {
        return Err(ParseError::new(
            number,
            "command template must not be empty",
        ));
    }
    let mut lines = Vec::with_capacity(texts.len());
    let mut stdout: Option<usize> = None;
    for (index, template) in texts.into_iter().enumerate() {
        let parsed = CommandTemplate::parse(template).map_err(|error| {
            ParseError::new(
                number,
                format!("command `{operation}`: {}", error.message()),
            )
            .at_token(template)
        })?;
        reject_shell_operators(template, &parsed, &what, number)?;
        check_redirects(template, &parsed, &what, role == CommandRole::Run, number)?;
        if let Some(redirect) = parsed
            .redirects()
            .iter()
            .find(|redirect| redirect.stream == Stream::Stdout)
        {
            if let Some(earlier) = stdout {
                return Err(ParseError::new(
                    number,
                    format!(
                        "{what} writes its stdout to an output on its line {} already; only one of a command's lines writes an output as its stdout",
                        earlier + 1
                    ),
                )
                .at_token(&template[redirect.at.clone()]));
            }
            stdout = Some(index);
        }
        lines.push(parsed);
    }
    Ok((
        CommandDef {
            operation: operation.to_owned(),
            lines,
            role,
        },
        places,
    ))
}

/// The lines of a command written as a block, `rest` being what follows
/// its `:` on the first line and each line beneath it, joined by newlines:
/// each line indented as the first starts a command, and one indented
/// further goes on with the command above it. Each command's text, and
/// where its first line is.
fn sequence_lines<'t>(
    rest: &'t str,
    number: usize,
    operation: &str,
) -> Result<(Vec<&'t str>, Vec<Place>), ParseError> {
    let mut texts: Vec<&'t str> = Vec::new();
    let mut places = Vec::new();
    // Where in `rest` the command being read starts and ends.
    let mut current: Option<(usize, usize)> = None;
    let mut indent = None;
    let mut offset = 0;
    for (index, segment) in rest.split('\n').enumerate() {
        let start = offset;
        offset += segment.len() + 1;
        let code = segment.trim();
        if index == 0 || code.is_empty() {
            continue;
        }
        let depth = segment.len() - segment.trim_start().len();
        let base = *indent.get_or_insert(depth);
        let end = start + segment.trim_end().len();
        if depth > base {
            if let Some((_, last)) = current.as_mut() {
                *last = end;
            }
            continue;
        }
        if depth < base {
            return Err(ParseError::new(
                number,
                format!(
                    "indent each line of the command for `{operation}` as its first, or further to go on with the line above"
                ),
            )
            .at_token(code));
        }
        if let Some((from, to)) = current.take() {
            texts.push(&rest[from..to]);
        }
        current = Some((start + depth, end));
        places.push(Place::new(number + index, content_columns(segment)));
    }
    if let Some((from, to)) = current {
        texts.push(&rest[from..to]);
    }
    Ok((texts, places))
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
