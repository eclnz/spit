use crate::model::{CommandDef, CommandRole};
use crate::span::{content_columns, Focus, Place};
use crate::template::{CommandTemplate, Stream};

use super::declarations::single_colon;
use super::error::CommandError;
use super::lexical::qualified_identifier;
use super::source_map::{tail_place, CommandPlaces};
use super::{ParseError, ParseErrorKind};

pub(super) fn parse_command(
    original: &str,
    line: &str,
    number: usize,
    role: CommandRole,
) -> Result<(CommandDef, CommandPlaces), ParseError> {
    let delimiter = declaration_delimiter(line).ok_or_else(|| {
        ParseError::of(
            number,
            match role {
                CommandRole::Run => CommandError::ExpectedCommandForm,
                CommandRole::Verify => CommandError::ExpectedVerifyForm,
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
    let first = rest.split('\n').next().unwrap_or_default();
    let (texts, places) = if first.trim().is_empty() && rest.contains('\n') {
        if role == CommandRole::Verify {
            return Err(ParseError::of(
                number,
                CommandError::VerifyHoldsOneCommand {
                    operation: operation.to_string(),
                },
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
        return Err(ParseError::of(number, CommandError::EmptyCommandTemplate));
    }
    let mut lines = Vec::with_capacity(texts.len());
    let mut stdout: Option<usize> = None;
    for (index, template) in texts.into_iter().enumerate() {
        let parsed = CommandTemplate::parse(template).map_err(|error| {
            let piece = match &error.location.focus {
                Some(Focus::Text(text)) => template
                    .find(&**text)
                    .map(|at| &template[at..at + text.len()]),
                _ => None,
            };
            ParseError::of(
                number,
                ParseErrorKind::CommandOf {
                    owner: format!("command `{operation}`"),
                    problem: error.error,
                },
            )
            .at_token(piece.unwrap_or(template))
        })?;
        reject_shell_operators(template, &parsed, &what, number)?;
        check_redirects(template, &parsed, &what, role == CommandRole::Run, number)?;
        if let Some(redirect) = parsed
            .redirects()
            .iter()
            .find(|redirect| redirect.stream == Stream::Stdout)
        {
            if let Some(earlier) = stdout {
                let line = earlier + 1;
                return Err(ParseError::of(
                    number,
                    CommandError::StdoutWrittenTwice {
                        what: what.to_string(),
                        line,
                    },
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

fn sequence_lines<'t>(
    rest: &'t str,
    number: usize,
    operation: &str,
) -> Result<(Vec<&'t str>, Vec<Place>), ParseError> {
    let mut texts: Vec<&'t str> = Vec::new();
    let mut places = Vec::new();
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
            return Err(ParseError::of(
                number,
                CommandError::CommandLineIndentation {
                    operation: operation.to_string(),
                },
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
    let error = if operator.contains(['<', '>']) && !operator.contains(['|', '&', ';']) {
        ParseError::of(
            number,
            CommandError::OperatorNotRedirection {
                operator: operator.to_string(),
                what: what.to_string(),
            },
        )
    } else {
        ParseError::of(
            number,
            CommandError::OperatorNotPipe {
                operator: operator.to_string(),
                what: what.to_string(),
            },
        )
    };
    Err(error.at_token(operator))
}

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
        let fail = |error: CommandError, at: &std::ops::Range<usize>| {
            Err(ParseError::of(number, error).at_token(&template[at.clone()]))
        };
        let (sign, what, stream, port, example) = (
            sign.to_string(),
            what.to_owned(),
            stream.to_owned(),
            port.to_owned(),
            example.to_owned(),
        );
        if !allowed {
            return fail(
                CommandError::RedirectNotAllowed {
                    sign,
                    what,
                    stream,
                    port,
                    example,
                },
                &redirect.at,
            );
        }
        if seen.contains(&redirect.stream) {
            return fail(
                CommandError::SecondRedirect { what, sign, stream },
                &redirect.at,
            );
        }
        seen.push(redirect.stream);
        match &redirect.target {
            None => {
                return fail(
                    CommandError::RedirectNeedsTarget {
                        sign,
                        what,
                        port,
                        stream,
                        example,
                    },
                    &redirect.at,
                )
            }
            Some((_, at)) if redirect.placeholder().is_none() => {
                return fail(
                    CommandError::RedirectTargetNotPlaceholder {
                        sign,
                        target: template[at.clone()].to_owned(),
                        what,
                        stream,
                        port,
                        example,
                    },
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
