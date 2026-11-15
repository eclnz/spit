//! Operation declarations: `name(inputs) -> outputs`, where a `many`
//! input may carry an `@min(count)` clause, and any port `@check(...)` and
//! `@review(...)`.

use crate::model::{
    Beside, Cardinality, InputPort, OperationDef, OutputPort, ShapeRule, DEFAULT_OUTPUT,
};
use crate::span::columns_of;
use crate::types::{parse_type_expr, TypeExpr};

use super::check::{split_clauses, Clause};
use super::declarations::type_error;
use super::error::OperationError;
use super::lexical::{call_parts, comma_items, identifier, split_ending};
use super::review::{attachments, is_attachment, only_attachments};
use super::scan::closing_paren;
use super::{ClauseSite, ParseError, ParseErrorKind};

pub(super) fn parse_operation(line: &str, number: usize) -> Result<OperationDef, ParseError> {
    let open = line
        .find('(')
        .filter(|&open| !line[..open].contains("->"))
        .ok_or_else(|| ParseError::of(number, OperationError::ExpectedAfterOperationName))?;
    // An unclosed signature runs to its `->`, for `call_parts` to report.
    let (signature, rest) = match closing_paren(line, open) {
        Some(close) => line.split_at(close + 1),
        None => line.split_at(line.find("->").unwrap_or(line.len())),
    };
    let signature = signature.trim_end();
    let rest = rest.trim();
    // Clauses after the outputs belong to the whole signature; those inside
    // the inputs, or after an output, stay with their ports.
    let (outputs, trailing) = if let Some(output) = rest.strip_prefix("->") {
        parse_outputs(output.trim(), number)?
    } else {
        if !rest.is_empty() && !rest.starts_with('@') {
            return Err(ParseError::of(number, OperationError::ExpectedArrow).at_token(rest));
        }
        (
            vec![OutputPort::new(DEFAULT_OUTPUT, TypeExpr::Unknown)],
            split_clauses(rest, ClauseSite::AfterSignature, number)?.1,
        )
    };
    if let Some(clause) = trailing.iter().find(|clause| is_attachment(clause)) {
        let keyword = clause.keyword;
        return Err(ParseError::of(
            number,
            OperationError::ClauseAfterWrongPlace {
                keyword: keyword.to_string(),
            },
        )
        .at_token(clause.text));
    }
    // The line without its trailing clauses, to rewrite a misplaced minimum.
    let line = match trailing.first() {
        Some(clause) => {
            let start = columns_of(line, clause.text).map_or(line.len(), |columns| columns.start);
            line[..start].trim_end().trim_end_matches('@').trim_end()
        }
        None => line,
    };
    let clauses = parse_clauses(&trailing, ClauseSite::AfterSignature, number)?;
    let (name, inputs) = call_parts(signature, number)?;
    identifier(name, number, "operation name")?;
    if let Some((dimensions, clause)) = clauses.drop {
        let vary = dimensions.join(", ");
        return Err(ParseError::of(
            number,
            OperationError::DropOnOperation {
                vary: vary.to_string(),
                name: name.to_string(),
            },
        )
        .at_token(clause));
    }
    let inputs = comma_items(inputs, number)?;
    if inputs.is_empty() {
        return Err(ParseError::of(
            number,
            OperationError::OperationNeedsLeastInput,
        ));
    }
    let mut minimum = None;
    let mut ports = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let (port, at_least) = parse_port_with_minimum(input, number)?;
        minimum = minimum.or(at_least);
        ports.push(port);
    }
    let shape_rule = shape_rule(&ports);
    check_beside(&ports, &outputs, line, number)?;
    if let Some((count, clause)) = clauses.min {
        return Err(misplaced_minimum(name, &inputs, &ports, line, count, number).at_token(clause));
    }
    let mut operation = OperationDef::with_outputs(name, ports, outputs, shape_rule);
    if let Some(minimum) = minimum {
        operation = operation.at_least(minimum);
    }
    Ok(operation)
}

/// An input port with its checks, and the minimum its `@min(count)`
/// clause sets, which only a `many` port may have.
fn parse_port_with_minimum(
    input: &str,
    number: usize,
) -> Result<(InputPort, Option<usize>), ParseError> {
    let (input, clauses) = split_clauses(input, ClauseSite::OnPort, number)?;
    let (attached, clauses): (Vec<_>, Vec<_>) = clauses.into_iter().partition(is_attachment);
    let mut port = parse_input_port(input, number)?;
    let attached = only_attachments(&attached, "an input port", number)?;
    port.checks = attached.checks;
    port.reviews = attached.reviews;
    if clauses.is_empty() {
        return Ok((port, None));
    }
    let clauses = parse_clauses(&clauses, ClauseSite::OnPort, number)?;
    if let Some((_, clause)) = clauses.drop {
        return Err(ParseError::of(
            number,
            ParseErrorKind::ExpectedClause {
                site: ClauseSite::OnPort,
            },
        )
        .at_token(clause));
    }
    let Some((count, clause)) = clauses.min else {
        unreachable!("a clause that is not `drop` is `min`, or parsing failed")
    };
    if port.cardinality != Cardinality::Many {
        let port_name = &port.name;
        return Err(ParseError::of(
            number,
            OperationError::MinCountGoesManyInputTakes {
                port_name: port_name.to_string(),
            },
        )
        .at_token(clause));
    }
    Ok((port, Some(count)))
}

/// The error for a minimum written after the signature: the line rewritten
/// with the minimum beside the `many` input it counts.
fn misplaced_minimum(
    name: &str,
    inputs: &[&str],
    ports: &[InputPort],
    line: &str,
    count: usize,
    number: usize,
) -> ParseError {
    let Some(many) = ports
        .iter()
        .position(|port| port.cardinality == Cardinality::Many)
    else {
        return ParseError::of(number, OperationError::MinCountRequiresManyInput);
    };
    let inputs: Vec<String> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let input = input.trim();
            if index == many {
                format!("{input} @min({count})")
            } else {
                input.to_owned()
            }
        })
        .collect();
    let outputs = line
        .split_once("->")
        .map(|(_, outputs)| format!(" -> {}", outputs.trim()))
        .unwrap_or_default();
    let list = inputs.join(", ");
    ParseError::of(
        number,
        OperationError::MinimumBesideInput {
            name: name.to_string(),
            list: list.to_string(),
            outputs: outputs.to_string(),
        },
    )
}

/// An `@min(count)` clause, and an `@drop(...)` clause, which is an error,
/// each with its text, kept to point at it and to say what to write instead.
#[derive(Default)]
struct Clauses<'a> {
    drop: Option<(Vec<String>, &'a str)>,
    min: Option<(usize, &'a str)>,
}

/// The meaning of `clauses`, which [`split_clauses`] split at each `@`;
/// `expected` says what else may be written where they are.
fn parse_clauses<'a>(
    clauses: &[Clause<'a>],
    site: ClauseSite,
    number: usize,
) -> Result<Clauses<'a>, ParseError> {
    let mut parsed = Clauses::default();
    for &Clause {
        keyword,
        argument,
        text: clause,
    } in clauses
    {
        match keyword {
            "drop" if parsed.drop.is_none() => {
                let dimensions = comma_items(argument, number)?;
                if dimensions.is_empty() {
                    return Err(
                        ParseError::of(number, OperationError::DropNeedsDimension).at_token(clause)
                    );
                }
                let mut names = Vec::new();
                for dimension in dimensions {
                    let dimension = identifier(dimension, number, "aggregated dimension")?;
                    if names.contains(&dimension.to_owned()) {
                        return Err(ParseError::of(
                            number,
                            OperationError::DropRepeats {
                                dimension: dimension.to_string(),
                            },
                        )
                        .at_token(clause));
                    }
                    names.push(dimension.to_owned());
                }
                parsed.drop = Some((names, clause));
            }
            "min" if parsed.min.is_none() => {
                let count: usize = argument
                    .parse()
                    .ok()
                    .filter(|count| *count > 0)
                    .ok_or_else(|| {
                        ParseError::of(number, OperationError::MinNeedsPositiveInteger)
                            .at_token(argument)
                    })?;
                parsed.min = Some((count, clause));
            }
            "drop" | "min" => {
                return Err(ParseError::of(
                    number,
                    OperationError::Duplicate {
                        keyword: keyword.to_string(),
                    },
                )
                .at_token(keyword))
            }
            _ => return Err(expected_clause(site, number).at_token(keyword)),
        }
    }
    Ok(parsed)
}

/// Parse an input port: `name`, `name: Type`, `name: many`, or
/// `name: many Type`.
fn parse_input_port(input: &str, number: usize) -> Result<InputPort, ParseError> {
    let fail =
        |error: OperationError, token: &str| Err(ParseError::of(number, error).at_token(token));
    let Some((name, value)) = input.split_once(':') else {
        // A lowercase word alone names an untyped port; types are capitalised.
        if let Some(rest) = input
            .strip_prefix("many ")
            .or_else(|| input.strip_prefix("one "))
        {
            let rest = rest.trim();
            let keyword = &input[..input.len() - rest.len()].trim();
            return if rest.starts_with(|c: char| c.is_ascii_lowercase()) {
                let kind = if *keyword == "many" { " many" } else { "" };
                fail(
                    OperationError::PortNameFirst {
                        rest: rest.to_owned(),
                        kind: kind.to_owned(),
                    },
                    input,
                )
            } else {
                let kind = if *keyword == "many" { "many " } else { "" };
                fail(
                    OperationError::PortNeedsName {
                        suggested: suggested_name(rest),
                        kind: kind.to_owned(),
                        rest: rest.to_owned(),
                    },
                    input,
                )
            };
        }
        return match input {
            "many" => fail(OperationError::ManyPortNeedsName, input),
            "one" => fail(OperationError::OnePortNeedsName, input),
            _ if input.starts_with(|c: char| c.is_ascii_lowercase()) => {
                Ok(InputPort::one(port_name(input, number)?, TypeExpr::Unknown))
            }
            _ => fail(
                OperationError::PortNeedsName {
                    suggested: suggested_name(input),
                    kind: String::new(),
                    rest: input.to_owned(),
                },
                input,
            ),
        };
    };
    let name = port_name(name, number)?;
    let value = value.trim();
    let (many, value) = match value {
        "many" => (true, ""),
        _ => match value.strip_prefix("many ") {
            Some(rest) => (true, rest.trim()),
            None => (false, value),
        },
    };
    if value == "one" || value.starts_with("one ") {
        let rest = value.strip_prefix("one").unwrap_or_default().trim();
        let instead = if rest.is_empty() {
            name.to_owned()
        } else {
            format!("{name}: {rest}")
        };
        return fail(OperationError::InputTakesOne { instead }, value);
    }
    let artifact_type = if value.is_empty() {
        TypeExpr::Unknown
    } else {
        port_type(value, number)?
    };
    Ok(if many {
        InputPort::many(name, artifact_type)
    } else {
        InputPort::one(name, artifact_type)
    })
}

/// A port's name, which `output` cannot be.
fn port_name(name: &str, number: usize) -> Result<&str, ParseError> {
    let name = identifier(name.trim(), number, "input port")?;
    if name == DEFAULT_OUTPUT {
        return Err(ParseError::of(number, OperationError::PortNameReserved {}).at_token(name));
    }
    Ok(name)
}

/// A name to suggest for a port typed `type_text`: its type's name, in
/// lowercase words joined by `_`.
fn suggested_name(type_text: &str) -> String {
    let head = type_text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default();
    let mut name = String::new();
    let mut previous: Option<char> = None;
    for character in head.chars() {
        // A word starts at a capital after a lowercase letter, so `MRI`
        // stays one word and `DenoisedBOLD` is two.
        if character.is_ascii_uppercase() && previous.is_some_and(|p| p.is_ascii_lowercase()) {
            name.push('_');
        }
        name.push(character.to_ascii_lowercase());
        previous = Some(character);
    }
    if name.is_empty() {
        "input".to_owned()
    } else {
        name
    }
}

/// Whether an operation aggregates, which it does with a many input.
fn shape_rule(ports: &[InputPort]) -> ShapeRule {
    let many = ports
        .iter()
        .any(|port| port.cardinality == Cardinality::Many);
    if many {
        ShapeRule::Aggregate
    } else {
        ShapeRule::Preserve
    }
}

/// Parse an operation's output: one type, or `(name: Type, ...)` for
/// several named outputs. Each type may be followed by the extension the
/// tool gives its file, as in `-> Transform .mat`, and then by its checks.
/// Also gives the operation's own clauses after the outputs.
fn parse_outputs(
    text: &str,
    number: usize,
) -> Result<(Vec<OutputPort>, Vec<Clause<'_>>), ParseError> {
    let Some(list) = text.strip_prefix('(') else {
        let (text, clauses) = split_clauses(text, ClauseSite::AfterOutput, number)?;
        // An output's checks are its own; anything else is the operation's.
        let (attached, clauses): (Vec<_>, Vec<_>) = clauses.into_iter().partition(is_attachment);
        let attached = attachments(&attached, "an output", number)?;
        let (text, beside) = split_beside(text, number)?;
        let (text, extension, folder) = match beside {
            Some(_) => (text, None, false),
            None => split_ending(text, number)?,
        };
        let output_type = if text.is_empty() {
            TypeExpr::Unknown
        } else {
            port_type(text, number)?
        };
        let port = OutputPort::new(DEFAULT_OUTPUT, output_type);
        let mut port = match beside {
            Some(beside) => port.beside(beside),
            None => with_ending(port, extension, folder),
        };
        port.checks = attached.checks;
        port.exempt = attached.exempt;
        port.reviews = attached.reviews;
        port.exempt_reviews = attached.exempt_reviews;
        return Ok((vec![port], clauses));
    };
    let closing = || ParseError::of(number, OperationError::UnclosedOutputPorts).at_token(text);
    let close = closing_paren(text, 0).ok_or_else(closing)?;
    let trailing = text[close + 1..].trim();
    if !trailing.is_empty() && !trailing.starts_with('@') {
        return Err(closing());
    }
    let clauses = split_clauses(trailing, ClauseSite::AfterSignature, number)?.1;
    let list = &list[..close - 1];
    let items = comma_items(list, number)?;
    if items.is_empty() {
        return Err(ParseError::of(number, OperationError::ExpectedLeastOutputPort).at_token(text));
    }
    let ports = items
        .into_iter()
        .map(|item| {
            let (item, clauses) = split_clauses(item, ClauseSite::AfterOutput, number)?;
            let attached = attachments(&clauses, "an output", number)?;
            let (item, beside) = split_beside(item, number)?;
            let (item, extension, folder) = match beside {
                Some(_) => (item, None, false),
                None => split_ending(item, number)?,
            };
            let (name, output_type) = match item.split_once(':') {
                Some((name, output_type)) if !output_type.trim().is_empty() => {
                    (name.trim(), port_type(output_type.trim(), number)?)
                }
                Some((name, _)) => (name.trim(), TypeExpr::Unknown),
                None => (item, TypeExpr::Unknown),
            };
            let name = identifier(name, number, "output port")?;
            if name == DEFAULT_OUTPUT {
                return Err(
                    ParseError::of(number, OperationError::OutputPortNameReserved).at_token(name),
                );
            }
            let mut port = OutputPort::new(name, output_type);
            port.checks = attached.checks;
            port.exempt = attached.exempt;
            port.reviews = attached.reviews;
            port.exempt_reviews = attached.exempt_reviews;
            Ok(match beside {
                Some(beside) => port.beside(beside),
                None => with_ending(port, extension, folder),
            })
        })
        .collect::<Result<Vec<_>, ParseError>>()?;
    Ok((ports, clauses))
}

/// An output's text before ` beside port`, and what `beside` says, if it
/// has one.
fn split_beside(item: &str, number: usize) -> Result<(&str, Option<Beside>), ParseError> {
    let Some((item, sibling)) = item.rsplit_once(" beside ") else {
        return Ok((item, None));
    };
    let (item, suffix, append) = beside_suffix(item.trim(), number, "output")?;
    let sibling = identifier(sibling.trim(), number, "port")?.to_owned();
    Ok((
        item,
        Some(Beside {
            sibling,
            suffix,
            append,
        }),
    ))
}

/// Check that each output written beside another names a port whose file
/// it can lie beside: another output the tool is told to write, or a
/// `one` input.
fn check_beside(
    inputs: &[InputPort],
    ports: &[OutputPort],
    line: &str,
    number: usize,
) -> Result<(), ParseError> {
    for port in ports {
        let Some(beside) = &port.beside else { continue };
        let (name, sibling) = (&port.name, &beside.sibling);
        let input = inputs.iter().find(|input| &input.name == sibling);
        let problem = match ports.iter().find(|other| &other.name == sibling) {
            Some(_) if name == sibling => OperationError::BesideItself { name: name.clone() },
            None => match input {
                Some(input) if input.cardinality == Cardinality::Many => {
                    OperationError::BesideManyInput {
                        name: name.clone(),
                        sibling: sibling.clone(),
                    }
                }
                Some(_) => continue,
                None => OperationError::BesideNeither {
                    name: name.clone(),
                    sibling: sibling.clone(),
                },
            },
            Some(other) if other.folder => OperationError::BesideFolder {
                name: name.clone(),
                sibling: sibling.clone(),
            },
            Some(other) if other.beside.is_some() => OperationError::BesideNested {
                name: name.clone(),
                sibling: sibling.clone(),
            },
            Some(other) if other.extension.is_none() && !beside.append => {
                OperationError::BesideNoExtension {
                    name: name.clone(),
                    sibling: sibling.clone(),
                }
            }
            Some(_) => continue,
        };
        let error = ParseError::of(number, problem);
        return Err(match named_beside(line, sibling) {
            Some(token) => error.at_token(token),
            None => error,
        });
    }
    Ok(())
}

/// The first `sibling` that `beside` names in `line`, as a slice of it.
fn named_beside<'a>(line: &'a str, sibling: &str) -> Option<&'a str> {
    line.match_indices(" beside ").find_map(|(at, keyword)| {
        let rest = line[at + keyword.len()..].trim_start();
        let start = line.len() - rest.len();
        let end = rest
            .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .unwrap_or(rest.len());
        (&rest[..end] == sibling).then(|| &line[start..start + end])
    })
}

/// A file written beside another: its text before the suffix, the suffix
/// its file name ends with, an extension such as `.json` or quoted text
/// such as `"_mask.nii.gz"`, and whether a `+` before it appends it to the
/// whole name, as `+.fai` does, rather than replacing the extension.
pub(super) fn beside_suffix<'a>(
    item: &'a str,
    number: usize,
    role: &str,
) -> Result<(&'a str, String, bool), ParseError> {
    let article = if role == "output" { "an" } else { "a" };
    if let Some(quoted) = item.strip_suffix('"') {
        let open = quoted.rfind('"').ok_or_else(|| {
            ParseError::of(number, OperationError::ExpectedOpeningSuffix).at_token(item)
        })?;
        let suffix = &quoted[open + 1..];
        let valid = !suffix.is_empty()
            && suffix
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character));
        if !valid {
            return Err(ParseError::of(
                number,
                OperationError::BadSuffix {
                    suffix: suffix.to_string(),
                },
            )
            .at_token(suffix));
        }
        let head = quoted[..open].trim_end();
        return Ok(match head.strip_suffix('+') {
            Some(head) => (head.trim(), suffix.to_owned(), true),
            None => (head.trim(), suffix.to_owned(), false),
        });
    }
    // `+.fai`: the `+` sits against the extension it appends.
    let (item, ending, append) = match item.find('.') {
        Some(dot) if item[..dot].trim_end().ends_with('+') => {
            let head = item[..dot].trim_end();
            (
                head[..head.len() - 1].trim(),
                split_ending(&item[dot..], number)?,
                true,
            )
        }
        _ => {
            let (item, extension, folder) = split_ending(item, number)?;
            (item, ("", extension, folder), false)
        }
    };
    match ending {
        (_, _, true) => Err(ParseError::of(
            number,
            OperationError::BesideFolderKind {
                article: article.to_string(),
                role: role.to_string(),
            },
        )
        .at_token(item)),
        (_, Some(extension), false) => Ok((item, extension.to_owned(), append)),
        (_, None, false) => Err(ParseError::of(
            number,
            OperationError::BesideNeedsSuffix {
                article: article.to_string(),
                role: role.to_string(),
                item: item.to_string(),
            },
        )
        .at_token(item)),
    }
}

fn with_ending(port: OutputPort, extension: Option<&str>, folder: bool) -> OutputPort {
    let mut port = match extension {
        Some(extension) => port.with_extension(extension),
        None => port,
    };
    port.folder = folder;
    port
}

fn port_type(text: &str, number: usize) -> Result<TypeExpr, ParseError> {
    parse_type_expr(text, true).map_err(|error| type_error(number, text, error))
}

/// The error for a clause written where `site` does not allow it.
pub(super) fn expected_clause(site: ClauseSite, number: usize) -> ParseError {
    ParseError::of(number, ParseErrorKind::ExpectedClause { site })
}
