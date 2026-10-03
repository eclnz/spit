//! Runtime checks: `check name(params): command` declarations, and the
//! `@ clause(...)` list a port or a source may end with, of which
//! `@ check(...)` attaches checks.

use crate::command::CommandTemplate;
use crate::model::{CheckDef, CheckUse};

use super::lexical::{comma_items, identifier, qualified_identifier};
use super::ParseError;

/// Parse a check declaration, the text after `check`: `nonempty: test -s
/// {@path}` or `ndim(n): check_ndim {@path} {n}`.
pub(super) fn parse_check(line: &str, number: usize) -> Result<CheckDef, ParseError> {
    let expected = "expected check name(parameters): executable {@path} [arguments]";
    let (head, template) = match line.find('(') {
        // Parameters come before the template's `:`.
        Some(open) if !line[..open].contains(':') => {
            let close = line[open..]
                .find(')')
                .map(|offset| open + offset)
                .ok_or_else(|| ParseError::new(number, expected).at_token(&line[open..]))?;
            let rest = line[close + 1..].trim_start();
            let template = rest
                .strip_prefix(':')
                .ok_or_else(|| ParseError::new(number, expected).at_token(line))?;
            (&line[..=close], template)
        }
        _ => line
            .split_once(':')
            .ok_or_else(|| ParseError::new(number, expected).at_token(line))?,
    };
    let (name, parameters) = match head.split_once('(') {
        Some((name, parameters)) => (name, Some(&parameters[..parameters.len() - 1])),
        None => (head, None),
    };
    let name = identifier(name.trim(), number, "check name")?;
    let mut names: Vec<String> = Vec::new();
    for parameter in comma_items(parameters.unwrap_or_default(), number)? {
        let parameter = identifier(parameter, number, "check parameter")?;
        if names.iter().any(|name| name == parameter) {
            return Err(ParseError::new(
                number,
                format!("check `{name}` repeats parameter `{parameter}`"),
            )
            .at_token(parameter));
        }
        names.push(parameter.to_owned());
    }
    if parameters.is_some_and(|parameters| parameters.trim().is_empty()) {
        return Err(ParseError::new(
            number,
            format!("check `{name}` takes no parameters, so it takes no parentheses; write `check {name}:`"),
        )
        .at_token(head));
    }
    let template = template.trim();
    if template.is_empty() {
        return Err(ParseError::new(number, "check command must not be empty").at_token(line));
    }
    let template = CommandTemplate::parse(template).map_err(|error| {
        ParseError::new(number, format!("check `{name}`: {}", error.message())).at_token(template)
    })?;
    Ok(CheckDef {
        name: name.to_owned(),
        parameters: names,
        template,
    })
}

/// One `@ keyword(argument)` clause after a port or a source, with its
/// whole text for errors.
pub(super) struct Clause<'a> {
    pub(super) keyword: &'a str,
    pub(super) argument: &'a str,
    pub(super) text: &'a str,
}

/// Split `text` at its first `@` outside brackets into what it declares
/// and the clauses after it, each `@ keyword(argument)`.
pub(super) fn split_clauses(
    text: &str,
    number: usize,
) -> Result<(&str, Vec<Clause<'_>>), ParseError> {
    let Some(at) = top_level_at(text) else {
        return Ok((text, Vec::new()));
    };
    let mut clauses = Vec::new();
    let mut rest = &text[at..];
    while let Some(after) = rest.strip_prefix('@') {
        let next = top_level_at(after).unwrap_or(after.len());
        let clause = after[..next].trim();
        let (keyword, argument) = clause
            .split_once('(')
            .and_then(|(keyword, rest)| Some((keyword.trim(), rest.strip_suffix(')')?.trim())))
            .ok_or_else(|| {
                ParseError::new(
                    number,
                    "expected `@ clause(...)`, as in `@ check(nonempty)`",
                )
                .at_token(clause)
            })?;
        clauses.push(Clause {
            keyword,
            argument,
            text: clause,
        });
        rest = &after[next..];
    }
    Ok((text[..at].trim_end(), clauses))
}

/// The first `@` in `text` outside parentheses, brackets and angles.
fn top_level_at(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth = depth.saturating_sub(1),
            '@' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

/// The checks a `@ check(...)` clause attaches: `nonempty, ndim(3)`.
pub(super) fn check_uses(clause: &Clause<'_>, number: usize) -> Result<Vec<CheckUse>, ParseError> {
    let items = comma_items(clause.argument, number)?;
    if items.is_empty() {
        return Err(ParseError::new(
            number,
            "`@ check(...)` names at least one check, as in `@ check(nonempty)`",
        )
        .at_token(clause.text));
    }
    items
        .into_iter()
        .map(|item| {
            let (name, arguments) = match item.split_once('(') {
                Some((name, rest)) => {
                    let arguments = rest.strip_suffix(')').ok_or_else(|| {
                        ParseError::new(number, "expected closing `)`").at_token(item)
                    })?;
                    (name.trim(), Some(arguments))
                }
                None => (item, None),
            };
            let name = qualified_identifier(name, number, "check name")?;
            let arguments = match arguments {
                Some(arguments) if arguments.trim().is_empty() => return Err(ParseError::new(
                    number,
                    format!(
                        "check `{name}` is given no arguments; write `{name}` without parentheses"
                    ),
                )
                .at_token(item)),
                Some(arguments) => comma_items(arguments, number)?
                    .into_iter()
                    .map(|argument| check_argument(argument, number))
                    .collect::<Result<_, _>>()?,
                None => Vec::new(),
            };
            Ok(CheckUse {
                check: name.to_owned(),
                arguments,
            })
        })
        .collect()
}

/// An argument to a check: one word, which the check's command gets as it
/// is written.
fn check_argument(argument: &str, number: usize) -> Result<String, ParseError> {
    let valid = !argument.is_empty()
        && !argument.contains(|c: char| c.is_whitespace() || "()\"'{}@,".contains(c));
    if !valid {
        return Err(ParseError::new(
            number,
            format!("`{argument}` cannot be a check argument; give one word without spaces, quotes, braces or parentheses, as in `ndim(3)`"),
        )
        .at_token(argument));
    }
    Ok(argument.to_owned())
}

/// The checks of `clauses`, which may only be `@ check(...)`; `what` names
/// where they are written for the error about any other clause.
pub(super) fn only_checks(
    clauses: &[Clause<'_>],
    what: &str,
    number: usize,
) -> Result<Vec<CheckUse>, ParseError> {
    let mut checks = Vec::new();
    for clause in clauses {
        if clause.keyword != "check" {
            return Err(ParseError::new(
                number,
                format!(
                    "{what} takes only `@ check(...)`, not `@ {}(...)`",
                    clause.keyword
                ),
            )
            .at_token(clause.text));
        }
        checks.extend(check_uses(clause, number)?);
    }
    Ok(checks)
}

#[cfg(test)]
mod tests {
    use super::{check_uses, parse_check, split_clauses};

    #[test]
    fn a_check_declares_its_parameters_and_command() {
        let check = parse_check("ndim(n, axis): check_ndim {@path} {n} {axis}", 1).unwrap();
        assert_eq!(check.name, "ndim");
        assert_eq!(check.parameters, ["n", "axis"]);
        assert_eq!(check.template, "check_ndim {@path} {n} {axis}");
        let check = parse_check("nonempty: test -s {@path}", 1).unwrap();
        assert!(check.parameters.is_empty());
        assert!(parse_check("nonempty(): test -s {@path}", 1).is_err());
        assert!(parse_check("ndim(n, n): x {@path}", 1).is_err());
        assert!(parse_check("ndim(n) x {@path}", 1).is_err());
        assert!(parse_check("nonempty:", 1).is_err());
    }

    #[test]
    fn clauses_split_after_the_port_and_name_their_checks() {
        let (port, clauses) =
            split_clauses("dwi: DWI @ check(ndim(4), nonempty) @ check(x)", 1).unwrap();
        assert_eq!(port, "dwi: DWI");
        assert_eq!(clauses.len(), 2);
        let uses = check_uses(&clauses[0], 1).unwrap();
        let written: Vec<_> = uses.iter().map(ToString::to_string).collect();
        assert_eq!(written, ["ndim(4)", "nonempty"]);
        let (port, clauses) = split_clauses("items: many Table<Image @ x>", 1).unwrap();
        assert_eq!(port, "items: many Table<Image @ x>");
        assert!(clauses.is_empty());
        let (_, clauses) = split_clauses("x @ check(a(\"b c\"))", 1).unwrap();
        assert!(check_uses(&clauses[0], 1).is_err());
        let (_, clauses) = split_clauses("x @ check()", 1).unwrap();
        assert!(check_uses(&clauses[0], 1).is_err());
    }
}
