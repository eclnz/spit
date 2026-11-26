use crate::model::{CheckDef, CheckKind, CheckUse};
use crate::template::CommandTemplate;

use super::command::{check_redirects, reject_shell_operators};
use super::error::{CheckError, LexicalError};
use super::lexical::{comma_items, identifier, qualified_identifier};
use super::operation::expected_clause;
use super::scan::{self, closing_paren, find_outside};
use super::{ClauseSite, ParseError, ParseErrorKind};

pub(super) fn parse_check(line: &str, number: usize) -> Result<CheckDef, ParseError> {
    let expected = || ParseError::of(number, CheckError::ExpectedCheckForm);
    let (head, template) = match line.find('(') {
        Some(open) if !line[..open].contains(':') => {
            let close =
                closing_paren(line, open).ok_or_else(|| expected().at_token(&line[open..]))?;
            let rest = line[close + 1..].trim_start();
            let template = rest
                .strip_prefix(':')
                .ok_or_else(|| expected().at_token(line))?;
            (&line[..=close], template)
        }
        _ => line
            .split_once(':')
            .ok_or_else(|| expected().at_token(line))?,
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
            return Err(ParseError::of(
                number,
                CheckError::RepeatedCheckParameter {
                    check: name.to_owned(),
                    parameter: parameter.to_owned(),
                },
            )
            .at_token(parameter));
        }
        names.push(parameter.to_owned());
    }
    if parameters.is_some_and(|parameters| parameters.trim().is_empty()) {
        return Err(ParseError::of(
            number,
            CheckError::CheckParenthesesWithoutParameters {
                check: name.to_owned(),
            },
        )
        .at_token(head));
    }
    if template
        .split('\n')
        .next()
        .is_some_and(|first| first.trim().is_empty())
        && !template.trim().is_empty()
    {
        return Err(ParseError::of(
            number,
            CheckError::CheckCommandOnNextLine {
                check: name.to_owned(),
            },
        )
        .at_token(head));
    }
    let template = template.trim();
    if template.is_empty() {
        return Err(ParseError::of(number, CheckError::EmptyCheckCommand).at_token(line));
    }
    let parsed = CommandTemplate::parse(template).map_err(|error| {
        ParseError::of(
            number,
            ParseErrorKind::CommandOf {
                owner: format!("check `{name}`"),
                problem: error.error,
            },
        )
        .at_token(template)
    })?;
    let what = format!("check `{name}`");
    reject_shell_operators(template, &parsed, &what, number)?;
    check_redirects(template, &parsed, &what, false, number)?;
    Ok(CheckDef {
        kind: CheckKind::Check,
        name: name.to_owned(),
        parameters: names,
        template: parsed,
    })
}

#[derive(Clone, Copy)]
pub(super) struct Clause<'a> {
    pub(super) keyword: &'a str,
    pub(super) argument: &'a str,
    pub(super) text: &'a str,
}

pub(super) fn split_clauses<'a>(
    text: &'a str,
    site: ClauseSite,
    number: usize,
) -> Result<(&'a str, Vec<Clause<'a>>), ParseError> {
    let Some(at) = top_level_at(text) else {
        return Ok((text, Vec::new()));
    };
    let mut clauses = Vec::new();
    let mut rest = &text[at..];
    while let Some(after) = rest.strip_prefix('@') {
        no_space_after_at(after, number)?;
        let next = top_level_at(after).unwrap_or(after.len());
        let clause = after[..next].trim();
        let (keyword, argument) = if matches!(site, ClauseSite::AfterCall)
            && clause == "review"
            && after.starts_with("review")
        {
            ("review", "")
        } else {
            clause
                .split_once('(')
                .and_then(|(keyword, rest)| Some((keyword.trim(), rest.strip_suffix(')')?.trim())))
                .ok_or_else(|| expected_clause(site, number).at_token(clause))?
        };
        clauses.push(Clause {
            keyword,
            argument,
            text: clause,
        });
        rest = &after[next..];
    }
    Ok((text[..at].trim_end(), clauses))
}

pub(super) fn no_space_after_at(after: &str, number: usize) -> Result<(), ParseError> {
    let clause = after.trim_start();
    if clause.len() == after.len() {
        return Ok(());
    }
    let keyword = clause.split(['(', ' ', '\t']).next().unwrap_or_default();
    let known = [
        "each", "vary", "where", "first", "last", "same", "min", "check", "review", "drop",
    ]
    .contains(&keyword);
    if known && clause[keyword.len()..].trim_start().starts_with('(') {
        return Err(ParseError::of(
            number,
            CheckError::SpaceAfterAt {
                keyword: keyword.to_owned(),
            },
        )
        .at_token(clause));
    }
    Ok(())
}

fn top_level_at(text: &str) -> Option<usize> {
    find_outside(text, scan::ALL, '@')
}

pub(super) fn check_uses(
    clause: &Clause<'_>,
    number: usize,
) -> Result<(Vec<CheckUse>, Vec<CheckUse>), ParseError> {
    let items = comma_items(clause.argument, number)?;
    if items.is_empty() {
        return Err(ParseError::of(number, CheckError::EmptyCheckClause).at_token(clause.text));
    }
    check_list(items, number)
}

pub(super) fn parse_default_checks(
    text: &str,
    number: usize,
) -> Result<(Vec<CheckUse>, Vec<CheckUse>), ParseError> {
    let items = comma_items(text, number)?;
    if items.is_empty() {
        return Err(ParseError::of(number, CheckError::EmptyDefaultChecks));
    }
    check_list(items, number)
}

fn check_list(
    items: Vec<&str>,
    number: usize,
) -> Result<(Vec<CheckUse>, Vec<CheckUse>), ParseError> {
    let mut uses = Vec::new();
    let mut exempt = Vec::new();
    for item in items {
        let (written, opt_out) = match item.strip_prefix('!') {
            Some(rest) => (rest.trim_start(), true),
            None => (item, false),
        };
        let used = check_use(written, item, number)?;
        if opt_out { &mut exempt } else { &mut uses }.push(used);
    }
    Ok((uses, exempt))
}

fn check_use(written: &str, item: &str, number: usize) -> Result<CheckUse, ParseError> {
    let (name, arguments) = match written.split_once('(') {
        Some((name, rest)) => {
            let arguments = rest.strip_suffix(')').ok_or_else(|| {
                ParseError::of(number, LexicalError::ExpectedCloseParen).at_token(item)
            })?;
            (name.trim(), Some(arguments))
        }
        None => (written, None),
    };
    let name = qualified_identifier(name, number, "check name")?;
    let arguments = match arguments {
        Some(arguments) if arguments.trim().is_empty() => {
            return Err(ParseError::of(
                number,
                CheckError::CheckWithoutArguments {
                    check: name.to_owned(),
                },
            )
            .at_token(item))
        }
        Some(arguments) => comma_items(arguments, number)?
            .into_iter()
            .map(|argument| check_argument(argument, number))
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    Ok(CheckUse {
        kind: CheckKind::Check,
        check: name.to_owned(),
        arguments,
    })
}

fn check_argument(argument: &str, number: usize) -> Result<String, ParseError> {
    let valid = !argument.is_empty()
        && !argument.contains(|c: char| c.is_whitespace() || "()\"'{}@,".contains(c));
    if !valid {
        return Err(ParseError::of(
            number,
            CheckError::InvalidCheckArgument {
                argument: argument.to_owned(),
            },
        )
        .at_token(argument));
    }
    Ok(argument.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{check_uses, parse_check, parse_default_checks, split_clauses};
    use crate::parser::ClauseSite;

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
    fn a_default_list_separates_the_checks_it_drops() {
        let (uses, exempt) = parse_default_checks("nonempty, ndim(3), !lines(2)", 1).unwrap();
        assert_eq!(uses.len(), 2);
        assert_eq!(exempt.len(), 1);
        assert_eq!(exempt[0].to_string(), "lines(2)");
        assert!(parse_default_checks("", 1).is_err());
        assert!(parse_default_checks("nonempty,, x", 1).is_err());
    }

    #[test]
    fn clauses_split_after_the_port_and_name_their_checks() {
        let (port, clauses) = split_clauses(
            "dwi: DWI @check(ndim(4), nonempty) @check(x)",
            ClauseSite::OnPort,
            1,
        )
        .unwrap();
        assert_eq!(port, "dwi: DWI");
        assert_eq!(clauses.len(), 2);
        let (uses, exempt) = check_uses(&clauses[0], 1).unwrap();
        assert!(exempt.is_empty());
        let written: Vec<_> = uses.iter().map(ToString::to_string).collect();
        assert_eq!(written, ["ndim(4)", "nonempty"]);
        let (port, clauses) =
            split_clauses("items: many Table<Image @ x>", ClauseSite::OnPort, 1).unwrap();
        assert_eq!(port, "items: many Table<Image @ x>");
        assert!(clauses.is_empty());
        let (_, clauses) = split_clauses("x @check(a(\"b c\"))", ClauseSite::OnPort, 1).unwrap();
        assert!(check_uses(&clauses[0], 1).is_err());
        let (_, clauses) = split_clauses("x @check()", ClauseSite::OnPort, 1).unwrap();
        assert!(check_uses(&clauses[0], 1).is_err());
    }
}
