//! A deliberately small parser for the v0.1 text format.

use std::collections::BTreeMap;
use std::fmt;

use crate::model::{
    Cardinality, CountRequirement, CoverageRule, EntityBinding, InputBinding, InputPort,
    Invocation, OperationDef, Pipeline, ProductDef, ShapeRule, SourceInventory, SourceRecord,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Section {
    Products,
    Operations,
    Pipeline,
    Constraints,
}

pub fn parse_pipeline(text: &str) -> Result<Pipeline, ParseError> {
    let mut pipeline = Pipeline::default();
    let mut section = None;

    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = original.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "products:" => section = Some(Section::Products),
            "operations:" => section = Some(Section::Operations),
            "pipeline:" => section = Some(Section::Pipeline),
            "constraints:" => section = Some(Section::Constraints),
            "sources:" | "contexts:" => {
                return Err(ParseError::new(
                    line_number,
                    "source inventory belongs in a separate file, not the pipeline definition",
                ))
            }
            _ => match section {
                Some(Section::Products) => {
                    pipeline.products.push(parse_product(line, line_number)?)
                }
                Some(Section::Operations) => pipeline
                    .operations
                    .push(parse_operation(line, line_number)?),
                Some(Section::Pipeline) => pipeline
                    .invocations
                    .push(parse_invocation(line, line_number)?),
                Some(Section::Constraints) => pipeline
                    .constraints
                    .push(parse_coverage_rule(line, line_number)?),
                None => return Err(ParseError::new(
                    line_number,
                    "expected a section header: products:, operations:, pipeline:, or constraints:",
                )),
            },
        }
    }

    Ok(pipeline)
}

/// Parse an inventory supplied by a dataset indexer or written as a fixture.
/// The inventory contains logical identities, never paths or artifact types.
pub fn parse_source_inventory(text: &str) -> Result<SourceInventory, ParseError> {
    let mut inventory = SourceInventory::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = original.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "sources:" => section = Some("sources"),
            "contexts:" => section = Some("contexts"),
            _ => match section {
                Some("sources") => inventory.artifacts.push(parse_source(line, number)?),
                Some("contexts") => inventory.contexts.push(parse_context(line, number)?),
                _ => {
                    return Err(ParseError::new(
                        number,
                        "expected inventory section header: sources: or contexts:",
                    ))
                }
            },
        }
    }
    Ok(inventory)
}

fn parse_coverage_rule(line: &str, number: usize) -> Result<CoverageRule, ParseError> {
    let syntax = "expected constraint: require product count=1 per [dimensions] or count>=1";
    let rest = line
        .strip_prefix("require ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let (subject, dimensions) = rest
        .split_once(" per ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let mut parts = subject.split_whitespace();
    let product = identifier(parts.next().unwrap_or(""), number, "constraint product")?;
    let count_token = parts
        .next()
        .ok_or_else(|| ParseError::new(number, syntax))?;
    if parts.next().is_some() {
        return Err(ParseError::new(number, syntax));
    }
    let count = if let Some(value) = count_token.strip_prefix("count=") {
        CountRequirement::Exactly(parse_count(value, number)?)
    } else if let Some(value) = count_token.strip_prefix("count>=") {
        CountRequirement::AtLeast(parse_count(value, number)?)
    } else {
        return Err(ParseError::new(number, syntax));
    };
    let dimensions = dimensions
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let dimensions = comma_items(dimensions, number)?;
    if dimensions.is_empty() {
        return Err(ParseError::new(
            number,
            "coverage rule needs group dimensions",
        ));
    }
    for dimension in &dimensions {
        identifier(dimension, number, "constraint dimension")?;
    }
    Ok(CoverageRule::new(
        product,
        &dimensions.iter().map(String::as_str).collect::<Vec<_>>(),
        count,
    ))
}

fn parse_count(value: &str, number: usize) -> Result<usize, ParseError> {
    value
        .parse()
        .map_err(|_| ParseError::new(number, "constraint count must be a nonnegative integer"))
}

fn parse_product(line: &str, number: usize) -> Result<ProductDef, ParseError> {
    let (name, rest) = line.split_once(':').ok_or_else(|| {
        ParseError::new(
            number,
            "expected product declaration: name : Type [dimensions]",
        )
    })?;
    let name = identifier(name.trim(), number, "product name")?;
    let (artifact_type, dimensions) = rest
        .trim()
        .split_once('[')
        .ok_or_else(|| ParseError::new(number, "expected product type followed by [dimensions]"))?;
    let artifact_type = identifier(artifact_type.trim(), number, "artifact type")?;
    let dimensions = dimensions
        .strip_suffix(']')
        .ok_or_else(|| ParseError::new(number, "expected closing `]` in product declaration"))?;
    let dimensions = comma_items(dimensions, number)?;
    for dimension in &dimensions {
        identifier(dimension, number, "dimension")?;
    }
    Ok(ProductDef::new(
        name,
        artifact_type,
        &dimensions.iter().map(String::as_str).collect::<Vec<_>>(),
    ))
}

fn parse_operation(line: &str, number: usize) -> Result<OperationDef, ParseError> {
    let (signature, output_type) = line.split_once("->").ok_or_else(|| {
        ParseError::new(
            number,
            "expected operation: name(InputType, ...) -> OutputType",
        )
    })?;
    let output_type = identifier(output_type.trim(), number, "output type")?;
    let (name, inputs) = call_parts(signature.trim(), number)?;
    let inputs = comma_items(inputs, number)?;
    if inputs.is_empty() {
        return Err(ParseError::new(
            number,
            "operation needs at least one input",
        ));
    }
    let mut ports = Vec::new();
    let count = inputs.len();
    for (index, input) in inputs.iter().enumerate() {
        let (cardinality, artifact_type) = if let Some(value) = input.strip_prefix("many ") {
            (Cardinality::Many, value.trim())
        } else {
            (Cardinality::One, input.as_str())
        };
        let artifact_type = identifier(artifact_type, number, "input type")?;
        let port_name = if count == 1 {
            "input".to_owned()
        } else {
            format!("input{}", index + 1)
        };
        ports.push(match cardinality {
            Cardinality::One => InputPort::one(&port_name, artifact_type),
            Cardinality::Many => InputPort::many(&port_name, artifact_type),
        });
    }
    let shape_rule = if ports
        .iter()
        .any(|port| port.cardinality == Cardinality::Many)
    {
        if ports.len() != 1 {
            return Err(ParseError::new(
                number,
                "v0.1 supports `many` only as an operation's sole input",
            ));
        }
        ShapeRule::Aggregate
    } else {
        ShapeRule::Preserve
    };
    Ok(OperationDef::new(name, ports, output_type, shape_rule))
}

fn parse_invocation(line: &str, number: usize) -> Result<Invocation, ParseError> {
    let (output_product, call) = line.split_once('=').ok_or_else(|| {
        ParseError::new(
            number,
            "expected pipeline invocation: output = operation(inputs)",
        )
    })?;
    let output_product = identifier(output_product.trim(), number, "output product")?;
    let (operation, args) = call_parts(call.trim(), number)?;
    let mut bindings = Vec::new();
    for arg in comma_items(args, number)? {
        if let Some((product, variation)) = arg.split_once('@') {
            let product = identifier(product.trim(), number, "input product")?;
            let (keyword, dimension) = call_parts(variation.trim(), number)?;
            if keyword != "vary" {
                return Err(ParseError::new(
                    number,
                    "only `@ vary(dimension)` is supported",
                ));
            }
            let dimension = identifier(dimension.trim(), number, "vary dimension")?;
            bindings.push(InputBinding::vary(product, dimension));
        } else {
            bindings.push(InputBinding::product(identifier(
                arg.trim(),
                number,
                "input product",
            )?));
        }
    }
    Ok(Invocation::new(operation, bindings, output_product))
}

fn parse_source(line: &str, number: usize) -> Result<SourceRecord, ParseError> {
    let (product, bindings) = line.split_once('[').ok_or_else(|| {
        ParseError::new(
            number,
            "expected source artifact: product[dimension=value,...]",
        )
    })?;
    let product = identifier(product.trim(), number, "source product")?;
    let entities = parse_bindings(bindings, number)?;
    Ok(SourceRecord::new(product, entities))
}

fn parse_context(line: &str, number: usize) -> Result<EntityBinding, ParseError> {
    let bindings = line
        .strip_prefix('[')
        .ok_or_else(|| ParseError::new(number, "expected context: [dimension=value,...]"))?;
    parse_bindings(bindings, number)
}

fn parse_bindings(bindings: &str, number: usize) -> Result<EntityBinding, ParseError> {
    let bindings = bindings
        .strip_suffix(']')
        .ok_or_else(|| ParseError::new(number, "expected closing `]` in source artifact"))?;
    let mut values = BTreeMap::new();
    for item in comma_items(bindings, number)? {
        let (dimension, value) = item.split_once('=').ok_or_else(|| {
            ParseError::new(number, "expected `dimension=value` in source artifact")
        })?;
        let dimension = identifier(dimension.trim(), number, "source dimension")?;
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(ParseError::new(
                number,
                "source dimension value must be one nonempty token",
            ));
        }
        if values
            .insert(dimension.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(ParseError::new(
                number,
                format!("duplicate source dimension `{dimension}`"),
            ));
        }
    }
    Ok(EntityBinding(values))
}

fn call_parts(line: &str, number: usize) -> Result<(&str, &str), ParseError> {
    let (name, args) = line
        .split_once('(')
        .ok_or_else(|| ParseError::new(number, "expected function-like call"))?;
    let name = identifier(name.trim(), number, "operation name")?;
    let args = args
        .strip_suffix(')')
        .ok_or_else(|| ParseError::new(number, "expected closing `)`"))?;
    Ok((name, args))
}

fn comma_items(text: &str, number: usize) -> Result<Vec<String>, ParseError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| ParseError::new(number, "unexpected `)`"))?
            }
            ',' if depth == 0 => {
                items.push(text[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(ParseError::new(number, "unclosed `(`"));
    }
    items.push(text[start..].trim().to_owned());
    if items.iter().any(String::is_empty) {
        return Err(ParseError::new(
            number,
            "empty item in comma-separated list",
        ));
    }
    Ok(items)
}

fn identifier<'a>(value: &'a str, number: usize, kind: &str) -> Result<&'a str, ParseError> {
    let mut characters = value.chars();
    let valid_first = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    if !valid_first
        || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(ParseError::new(
            number,
            format!("invalid {kind} `{value}`; use letters, digits, and underscores"),
        ));
    }
    Ok(value)
}
