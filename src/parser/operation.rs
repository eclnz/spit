//! Operation declarations: `name(inputs) -> outputs`, with optional
//! `@ drop(dimension)` and `@ min(count)` clauses.

use crate::model::{
    Cardinality, DefaultPort, InputPort, OperationDef, OutputPort, ShapeRule, DEFAULT_OUTPUT,
};
use crate::types::{parse_type_expr, TypeExpr};

use super::declarations::type_error;
use super::lexical::{call_parts, comma_items, identifier};
use super::ParseError;

pub(super) fn parse_operation(line: &str, number: usize) -> Result<OperationDef, ParseError> {
    let mut clauses = line.split('@');
    let line = clauses.next().unwrap_or_default().trim_end();
    let clauses = parse_clauses(clauses, number)?;
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
    let inputs = comma_items(inputs, number)?;
    if inputs.is_empty() {
        return Err(ParseError::new(
            number,
            "operation needs at least one input",
        ));
    }
    let ports = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| parse_input_port(input, index, inputs.len(), number))
        .collect::<Result<Vec<_>, _>>()?;
    let shape_rule = shape_rule(&ports, &clauses, number)?;
    let mut operation = OperationDef::with_outputs(name, ports, outputs, shape_rule);
    if let Some(dimension) = clauses.drop {
        operation = operation.aggregating(dimension);
    }
    if let Some(minimum) = clauses.min {
        operation = operation.at_least(minimum);
    }
    Ok(operation)
}

/// The `@ drop(dimension)` and `@ min(count)` clauses after a signature.
#[derive(Default)]
struct Clauses<'a> {
    drop: Option<&'a str>,
    min: Option<usize>,
}

fn parse_clauses<'a>(
    clauses: impl Iterator<Item = &'a str>,
    number: usize,
) -> Result<Clauses<'a>, ParseError> {
    let expected = "expected `@ drop(dimension)` or `@ min(count)` after operation signature";
    let mut parsed = Clauses::default();
    for clause in clauses {
        let clause = clause.trim();
        let (keyword, argument) = clause
            .split_once('(')
            .and_then(|(keyword, rest)| Some((keyword.trim(), rest.strip_suffix(')')?.trim())))
            .ok_or_else(|| ParseError::new(number, expected).at_token(clause))?;
        match keyword {
            "drop" if parsed.drop.is_none() => {
                parsed.drop = Some(identifier(argument, number, "aggregated dimension")?);
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
                parsed.min = Some(count);
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

/// Parse input `index` of `count`: `[name:] [one|many] [Type]`.
fn parse_input_port(
    input: &str,
    index: usize,
    count: usize,
    number: usize,
) -> Result<InputPort, ParseError> {
    let (declared_name, input) = if let Some((name, value)) = input.split_once(':') {
        let name = identifier(name.trim(), number, "input port")?;
        if name == DEFAULT_OUTPUT {
            return Err(ParseError::new(
                number,
                format!("input port name `{DEFAULT_OUTPUT}` is reserved for the operation output"),
            )
            .at_token(name));
        }
        (Some(name), value.trim())
    } else {
        (None, input)
    };
    let (cardinality, artifact_type) = if input == "many" {
        (Cardinality::Many, TypeExpr::Unknown)
    } else if input == "one" {
        (Cardinality::One, TypeExpr::Unknown)
    } else if let Some(value) = input.strip_prefix("many ") {
        (Cardinality::Many, port_type(value.trim(), number)?)
    } else if let Some(value) = input.strip_prefix("one ") {
        (Cardinality::One, port_type(value.trim(), number)?)
    } else {
        (Cardinality::One, port_type(input, number)?)
    };
    let port_name = declared_name
        .map(str::to_owned)
        .unwrap_or_else(|| DefaultPort::for_input(index, count).name());
    Ok(match cardinality {
        Cardinality::One => InputPort::one(&port_name, artifact_type),
        Cardinality::Many => InputPort::many(&port_name, artifact_type),
    })
}

/// Whether an operation aggregates, which it does with one many input; only
/// then may it drop a dimension or require a minimum count.
fn shape_rule(
    ports: &[InputPort],
    clauses: &Clauses<'_>,
    number: usize,
) -> Result<ShapeRule, ParseError> {
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
    if many == 1 {
        return Ok(ShapeRule::Aggregate);
    }
    if clauses.drop.is_some() {
        return Err(ParseError::new(
            number,
            "`@ drop(dimension)` requires a many input",
        ));
    }
    if clauses.min.is_some() {
        return Err(ParseError::new(
            number,
            "`@ min(count)` requires a many input",
        ));
    }
    Ok(ShapeRule::Preserve)
}

/// Parse an operation's output: one type, or `(name: Type, ...)` for
/// several named outputs.
fn parse_outputs(text: &str, number: usize) -> Result<Vec<OutputPort>, ParseError> {
    let Some(list) = text.strip_prefix('(') else {
        let output_type =
            parse_type_expr(text, true).map_err(|error| type_error(number, text, error))?;
        return Ok(vec![OutputPort::new(DEFAULT_OUTPUT, output_type)]);
    };
    let list = list.strip_suffix(')').ok_or_else(|| {
        ParseError::new(number, "expected closing `)` after output ports").at_token(text)
    })?;
    let items = comma_items(list, number)?;
    if items.is_empty() {
        return Err(ParseError::new(number, "expected at least one output port").at_token(text));
    }
    items
        .into_iter()
        .map(|item| {
            let (name, output_type) = match item.split_once(':') {
                Some((name, output_type)) => (name.trim(), port_type(output_type.trim(), number)?),
                None => (item, TypeExpr::Unknown),
            };
            Ok(OutputPort::new(
                identifier(name, number, "output port")?,
                output_type,
            ))
        })
        .collect()
}

fn port_type(text: &str, number: usize) -> Result<TypeExpr, ParseError> {
    parse_type_expr(text, true).map_err(|error| type_error(number, text, error))
}
