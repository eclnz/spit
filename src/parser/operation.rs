//! Operation declarations: `name(inputs) -> outputs`, where a `many`
//! input may carry an `@ min(count)` clause.

use crate::model::{Cardinality, InputPort, OperationDef, OutputPort, ShapeRule, DEFAULT_OUTPUT};
use crate::types::{parse_type_expr, TypeExpr};

use super::declarations::type_error;
use super::lexical::{call_parts, comma_items, identifier, split_ending};
use super::ParseError;

pub(super) fn parse_operation(line: &str, number: usize) -> Result<OperationDef, ParseError> {
    // Clauses after the inputs belong to the whole signature; those inside
    // them stay with their ports.
    let inputs_end = inputs_end(line).unwrap_or(0);
    let cut = line[inputs_end..]
        .find('@')
        .map_or(line.len(), |at| inputs_end + at);
    let (line, trailing) = line.split_at(cut);
    let line = line.trim_end();
    let clauses = parse_clauses(trailing.split('@').skip(1), TRAILING, number)?;
    let (signature, outputs) = if let Some((signature, output)) = line.split_once("->") {
        (signature, parse_outputs(output.trim(), number)?)
    } else {
        (
            line,
            vec![OutputPort::new(DEFAULT_OUTPUT, TypeExpr::Unknown)],
        )
    };
    let signature = signature.trim();
    if !signature.contains('(') {
        return Err(ParseError::new(number, "expected `(` after operation name"));
    }
    if !line.contains("->") {
        if let Some((_, trailing)) = signature.split_once(')') {
            let trailing = trailing.trim();
            if !trailing.is_empty() {
                return Err(
                    ParseError::new(number, "expected `->` before operation output type")
                        .at_token(trailing),
                );
            }
        }
    }
    let (name, inputs) = call_parts(signature, number)?;
    identifier(name, number, "operation name")?;
    if let Some((dimensions, clause)) = clauses.drop {
        let vary = dimensions.join(", ");
        return Err(ParseError::new(
            number,
            format!(
                "an operation no longer names the dimensions it collects; remove `@ drop({vary})` \
                 and write `@ vary({vary})` on the many input of each call, as in \
                 `result = {name}(input @ vary({vary}))`"
            ),
        )
        .at_token(clause));
    }
    let inputs = comma_items(inputs, number)?;
    if inputs.is_empty() {
        return Err(ParseError::new(
            number,
            "operation needs at least one input",
        ));
    }
    let mut minimum = None;
    let mut ports = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let (port, at_least) = parse_port_with_minimum(input, number)?;
        minimum = minimum.or(at_least);
        ports.push(port);
    }
    let shape_rule = shape_rule(&ports, number)?;
    if let Some((count, clause)) = clauses.min {
        return Err(misplaced_minimum(name, &inputs, &ports, line, count, number).at_token(clause));
    }
    let mut operation = OperationDef::with_outputs(name, ports, outputs, shape_rule);
    if let Some(minimum) = minimum {
        operation = operation.at_least(minimum);
    }
    Ok(operation)
}

/// One past the `)` that closes the input list, so an `@` clause inside it
/// stays with its port, or `None` if the list is never opened or closed.
fn inputs_end(line: &str) -> Option<usize> {
    let open = line.find('(')?;
    let mut depth = 0_usize;
    for (index, character) in line[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// An input port and the minimum its `@ min(count)` clause sets, which
/// only a `many` port may have.
fn parse_port_with_minimum(
    input: &str,
    number: usize,
) -> Result<(InputPort, Option<usize>), ParseError> {
    let Some((port, clauses)) = input.split_once('@') else {
        return Ok((parse_input_port(input, number)?, None));
    };
    let clauses = parse_clauses(clauses.split('@'), ON_PORT, number)?;
    if let Some((_, clause)) = clauses.drop {
        return Err(ParseError::new(number, ON_PORT).at_token(clause));
    }
    let port = parse_input_port(port.trim_end(), number)?;
    let Some((count, clause)) = clauses.min else {
        unreachable!("a clause that is not `drop` is `min`, or parsing failed")
    };
    if port.cardinality != Cardinality::Many {
        return Err(ParseError::new(
            number,
            format!(
                "`@ min(count)` goes on a `many` input; `{}` takes one artifact",
                port.name
            ),
        )
        .at_token(clause));
    }
    Ok((port, Some(count)))
}

/// The error for a minimum written after the signature: the line as it is
/// written now, with the minimum beside the `many` input it counts.
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
        return ParseError::new(number, "`@ min(count)` requires a many input");
    };
    let inputs: Vec<String> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let input = input.trim();
            if index == many {
                format!("{input} @ min({count})")
            } else {
                input.to_owned()
            }
        })
        .collect();
    let outputs = line
        .split_once("->")
        .map(|(_, outputs)| format!(" -> {}", outputs.trim()))
        .unwrap_or_default();
    ParseError::new(
        number,
        format!(
            "write a many input's minimum beside it: `{name}({}){outputs}`",
            inputs.join(", ")
        ),
    )
}

/// What a clause after the signature may be: none is now, but `min` and
/// `drop`, which were, are read to say what to write instead.
const TRAILING: &str =
    "nothing follows an operation's outputs; write `@ min(count)` on its many input";

/// What a clause on an input port may be.
const ON_PORT: &str = "expected `@ min(count)` on a many input";

/// An `@ min(count)` clause, and a removed `@ drop(...)` clause, each with
/// its text, kept to point at it and to say what to write instead.
#[derive(Default)]
struct Clauses<'a> {
    drop: Option<(Vec<String>, &'a str)>,
    min: Option<(usize, &'a str)>,
}

/// The clauses split at each `@`; `expected` says what else may be written
/// where they are.
fn parse_clauses<'a>(
    clauses: impl Iterator<Item = &'a str>,
    expected: &str,
    number: usize,
) -> Result<Clauses<'a>, ParseError> {
    let mut parsed = Clauses::default();
    for clause in clauses {
        let clause = clause.trim();
        let (keyword, argument) = clause
            .split_once('(')
            .and_then(|(keyword, rest)| Some((keyword.trim(), rest.strip_suffix(')')?.trim())))
            .ok_or_else(|| ParseError::new(number, expected).at_token(clause))?;
        match keyword {
            "drop" if parsed.drop.is_none() => {
                let dimensions = comma_items(argument, number)?;
                if dimensions.is_empty() {
                    return Err(
                        ParseError::new(number, "`@ drop(...)` needs a dimension").at_token(clause)
                    );
                }
                let mut names = Vec::new();
                for dimension in dimensions {
                    let dimension = identifier(dimension, number, "aggregated dimension")?;
                    if names.contains(&dimension.to_owned()) {
                        return Err(ParseError::new(
                            number,
                            format!("`@ drop(...)` repeats `{dimension}`"),
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
                        ParseError::new(number, "`@ min(count)` needs a positive integer")
                            .at_token(argument)
                    })?;
                parsed.min = Some((count, clause));
            }
            "drop" | "min" => {
                return Err(
                    ParseError::new(number, format!("duplicate `@ {keyword}(...)`"))
                        .at_token(keyword),
                )
            }
            _ => return Err(ParseError::new(number, expected).at_token(keyword)),
        }
    }
    Ok(parsed)
}

/// Parse an input port: `name`, `name: Type`, `name: many`, or
/// `name: many Type`.
fn parse_input_port(input: &str, number: usize) -> Result<InputPort, ParseError> {
    let fail = |message: String, token: &str| Err(ParseError::new(number, message).at_token(token));
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
                    format!("write `{rest}:{kind}`, the port's name first"),
                    input,
                )
            } else {
                let kind = if *keyword == "many" { "many " } else { "" };
                fail(
                    format!(
                        "an input port needs a name, as in `{}: {kind}{rest}`",
                        suggested_name(rest)
                    ),
                    input,
                )
            };
        }
        return match input {
            "many" => fail(
                "an input port needs a name, as in `items: many`".to_owned(),
                input,
            ),
            "one" => fail("an input port needs a name, as in `item`".to_owned(), input),
            _ if input.starts_with(|c: char| c.is_ascii_lowercase()) => {
                Ok(InputPort::one(port_name(input, number)?, TypeExpr::Unknown))
            }
            _ => fail(
                format!(
                    "an input port needs a name, as in `{}: {input}`",
                    suggested_name(input)
                ),
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
        return fail(
            format!("an input takes one artifact unless it says `many`; write `{instead}`"),
            value,
        );
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
        return Err(ParseError::new(
            number,
            format!("input port name `{DEFAULT_OUTPUT}` is reserved for the operation output"),
        )
        .at_token(name));
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

/// Whether an operation aggregates, which it does with one many input.
fn shape_rule(ports: &[InputPort], number: usize) -> Result<ShapeRule, ParseError> {
    let many = ports
        .iter()
        .filter(|port| port.cardinality == Cardinality::Many)
        .count();
    if many > 1 {
        return Err(ParseError::new(
            number,
            "an operation takes at most one `many` input; each job groups one collection",
        ));
    }
    Ok(if many == 1 {
        ShapeRule::Aggregate
    } else {
        ShapeRule::Preserve
    })
}

/// Parse an operation's output: one type, or `(name: Type, ...)` for
/// several named outputs. Each type may be followed by the extension the
/// tool gives its file: `-> Transform .mat`.
fn parse_outputs(text: &str, number: usize) -> Result<Vec<OutputPort>, ParseError> {
    let Some(list) = text.strip_prefix('(') else {
        if let Some((_, port)) = text.rsplit_once(" beside ") {
            return Err(ParseError::new(
                number,
                "`beside` names another output of the same operation; name this one, as in `-> (image: Image .nii.gz, meta: Json .json beside image)`",
            )
            .at_token(port.trim()));
        }
        let (text, extension, folder) = split_ending(text, number)?;
        let output_type = if text.is_empty() {
            TypeExpr::Unknown
        } else {
            port_type(text, number)?
        };
        return Ok(vec![with_ending(
            OutputPort::new(DEFAULT_OUTPUT, output_type),
            extension,
            folder,
        )]);
    };
    let list = list.strip_suffix(')').ok_or_else(|| {
        ParseError::new(number, "expected closing `)` after output ports").at_token(text)
    })?;
    let items = comma_items(list, number)?;
    if items.is_empty() {
        return Err(ParseError::new(number, "expected at least one output port").at_token(text));
    }
    let ports = items
        .into_iter()
        .map(|item| {
            let (item, beside) = match item.rsplit_once(" beside ") {
                Some((item, sibling)) => {
                    let (item, suffix) = beside_suffix(item.trim(), number)?;
                    (
                        item,
                        Some((identifier(sibling.trim(), number, "output port")?, suffix)),
                    )
                }
                None => (item, None),
            };
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
                return Err(ParseError::new(
                    number,
                    "named output port `output` is reserved for the single unnamed output; omit the name and parentheses",
                )
                .at_token(name));
            }
            let port = OutputPort::new(name, output_type);
            Ok(match beside {
                Some((sibling, suffix)) => port.beside(sibling, suffix),
                None => with_ending(port, extension, folder),
            })
        })
        .collect::<Result<Vec<_>, ParseError>>()?;
    for port in &ports {
        let Some(beside) = &port.beside else { continue };
        let (name, sibling) = (&port.name, &beside.port);
        let problem = match ports.iter().find(|other| &other.name == sibling) {
            None => format!("`{name}` is written beside `{sibling}`, which is not an output of this operation"),
            Some(other) if other.folder => format!(
                "`{name}` is written beside `{sibling}`, which is a folder, and only a file has files beside it; drop `beside {sibling}` and give the tool `{{{name}}}`"
            ),
            Some(other) if other.beside.is_some() => format!(
                "`{name}` is written beside `{sibling}`, which is itself written beside another; name an output the tool is told to write"
            ),
            Some(other) if other.extension.is_none() => format!(
                "`{name}` is written beside `{sibling}`, which declares no extension for `{name}` to replace; give `{sibling}` one, as in `{sibling}: Image .nii.gz`"
            ),
            Some(_) => continue,
        };
        return Err(ParseError::new(number, problem).at_token(sibling));
    }
    Ok(ports)
}

/// An output written beside another: its text before the suffix, and the
/// suffix its file name ends with, an extension such as `.json` or quoted
/// text such as `"_mask.nii.gz"`.
fn beside_suffix(item: &str, number: usize) -> Result<(&str, String), ParseError> {
    if let Some(quoted) = item.strip_suffix('"') {
        let open = quoted.rfind('"').ok_or_else(|| {
            ParseError::new(number, "expected the opening `\"` of the suffix").at_token(item)
        })?;
        let suffix = &quoted[open + 1..];
        let valid = !suffix.is_empty()
            && suffix
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character));
        if !valid {
            return Err(ParseError::new(
                number,
                format!("`\"{suffix}\"` cannot end a file name; use letters, digits, `.`, `-` or `_`, as in `\"_mask.nii.gz\"`"),
            )
            .at_token(suffix));
        }
        return Ok((quoted[..open].trim(), suffix.to_owned()));
    }
    match split_ending(item, number)? {
        (item, _, true) => Err(ParseError::new(
            number,
            "an output written beside another is a file, not a folder; drop the `/`",
        )
        .at_token(item)),
        (item, Some(extension), false) => Ok((item, extension.to_owned())),
        (item, None, false) => Err(ParseError::new(
            number,
            format!("an output written beside another names what its file name ends with, as in `{item} .json beside image` or `{item} \"_mask.nii.gz\" beside image`"),
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
