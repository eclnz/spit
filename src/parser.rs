//! Parse sectioned or flow-style pipelines and separate source inventories.

use std::collections::BTreeMap;
use std::fmt;

use crate::imports::apply_import;
use crate::model::{
    Cardinality, CommandDef, CountRequirement, CoverageRule, EntityBinding, InputBinding,
    InputPort, Invocation, OperationDef, Pipeline, ProductDef, ShapeRule, SourceInventory,
    SourceRecord,
};
use crate::types::{parse_type_expr, TypeExpr};

const SHELL_SOURCE_REMOVED: &str =
    "shell-source is no longer supported; make the command executable available on PATH";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
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
    Commands,
}

pub fn parse_pipeline(text: &str) -> Result<Pipeline, ParseError> {
    parse_pipeline_with_imports(text, &BTreeMap::new())
}

fn parse_pipeline_with_imports(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
) -> Result<Pipeline, ParseError> {
    if is_sectioned_document(text) {
        parse_sectioned_pipeline(text, imports)
    } else {
        parse_flow_pipeline(text, imports)
    }
}

fn is_sectioned_document(text: &str) -> bool {
    text.lines().map(strip_comment).map(str::trim).any(|line| {
        matches!(
            line,
            "products:" | "operations:" | "pipeline:" | "constraints:" | "commands:"
        )
    })
}

fn parse_sectioned_pipeline(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
) -> Result<Pipeline, ParseError> {
    let mut pipeline = Pipeline::default();
    let mut section = None;

    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "products:" => section = Some(Section::Products),
            "operations:" => section = Some(Section::Operations),
            "pipeline:" => section = Some(Section::Pipeline),
            "constraints:" => section = Some(Section::Constraints),
            "commands:" => section = Some(Section::Commands),
            source if source.starts_with("use ") => {
                apply_import(&mut pipeline, imports, line_number)?;
                section = None;
            }
            source if source.starts_with("shell-source:") => {
                return Err(ParseError::new(line_number, SHELL_SOURCE_REMOVED));
            }
            path if path.starts_with("path:") || path.starts_with("path ") => {
                set_path(&mut pipeline, path, line_number)?;
                section = None;
            }
            "sources:" | "contexts:" => {
                return Err(ParseError::new(
                    line_number,
                    "source inventory is separate from Pipeline; use parse_document for a combined text file",
                ))
            }
            _ => match section {
                Some(Section::Products) => {
                    let product = parse_product(line, line_number)?;
                    pipeline
                        .source_lines
                        .products
                        .insert(product.name.clone(), line_number);
                    pipeline.products.push(product);
                }
                Some(Section::Operations) => {
                    let operation = parse_operation(line, line_number)?;
                    pipeline
                        .source_lines
                        .operations
                        .insert(operation.name.clone(), line_number);
                    pipeline.operations.push(operation);
                }
                Some(Section::Pipeline) => {
                    let invocation = parse_invocation(line, line_number)?;
                    pipeline
                        .source_lines
                        .invocations
                        .insert(invocation.output_product.clone(), line_number);
                    pipeline.invocations.push(invocation);
                }
                Some(Section::Constraints) => {
                    let constraint = parse_coverage_rule(line, line_number)?;
                    pipeline
                        .source_lines
                        .constraints
                        .insert(constraint.product.clone(), line_number);
                    pipeline.source_lines.constraint_lines.push(line_number);
                    pipeline.constraints.push(constraint);
                }
                Some(Section::Commands) => pipeline
                    .commands
                    .push(parse_command(line, line_number)?),
                None => return Err(ParseError::new(
                    line_number,
                    "expected a section header: products:, operations:, pipeline:, constraints:, or commands:",
                )),
            },
        }
    }

    Ok(pipeline)
}

fn parse_flow_pipeline(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
) -> Result<Pipeline, ParseError> {
    let mut pipeline = Pipeline::default();

    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("use ") {
            apply_import(&mut pipeline, imports, line_number)?;
        } else if let Some(declaration) = line.strip_prefix("source ") {
            let product = parse_product(declaration.trim(), line_number)?;
            pipeline
                .source_lines
                .products
                .insert(product.name.clone(), line_number);
            pipeline.products.push(product);
        } else if line.starts_with("operation ") {
            let declaration = line.strip_prefix("operation ").unwrap().trim();
            let operation = parse_operation(declaration, line_number)?;
            pipeline
                .source_lines
                .operations
                .insert(operation.name.clone(), line_number);
            pipeline.operations.push(operation);
        } else if line.starts_with("require ") {
            let constraint = parse_coverage_rule(line, line_number)?;
            pipeline
                .source_lines
                .constraints
                .insert(constraint.product.clone(), line_number);
            pipeline.source_lines.constraint_lines.push(line_number);
            pipeline.constraints.push(constraint);
        } else if line.starts_with("command ") {
            let declaration = line.strip_prefix("command ").unwrap().trim();
            pipeline
                .commands
                .push(parse_command(declaration, line_number)?);
        } else if line.starts_with("shell-source:") {
            return Err(ParseError::new(line_number, SHELL_SOURCE_REMOVED));
        } else if line.starts_with("path ") || line.starts_with("path:") {
            set_path(&mut pipeline, line, line_number)?;
        } else if line.contains('=') {
            let (invocation, product) = parse_flow_invocation(line, line_number, &pipeline)?;
            pipeline
                .source_lines
                .products
                .insert(product.name.clone(), line_number);
            pipeline
                .source_lines
                .invocations
                .insert(invocation.output_product.clone(), line_number);
            pipeline.products.push(product);
            pipeline.invocations.push(invocation);
        } else if line.contains('(') && line.ends_with(')') {
            return Err(ParseError::new(
                line_number,
                "expected `=` before operation call",
            ));
        } else {
            return Err(ParseError::new(
                line_number,
                "expected source, operation, require, or output = operation(inputs)",
            ));
        }
    }

    Ok(pipeline)
}

fn parse_command(line: &str, number: usize) -> Result<CommandDef, ParseError> {
    let delimiter = declaration_delimiter(line).ok_or_else(|| {
        ParseError::new(
            number,
            "expected command: operation: executable [arguments]",
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
    Ok(CommandDef::new(operation, template))
}

fn declaration_delimiter(line: &str) -> Option<usize> {
    [single_colon(line), line.find('=')]
        .into_iter()
        .flatten()
        .min()
}

fn single_colon(line: &str) -> Option<usize> {
    line.char_indices().find_map(|(index, character)| {
        (character == ':' && !line[..index].ends_with(':') && !line[index + 1..].starts_with(':'))
            .then_some(index)
    })
}

fn set_path(pipeline: &mut Pipeline, line: &str, number: usize) -> Result<(), ParseError> {
    let (product, template) = if let Some(template) = line.strip_prefix("path:") {
        (None, template)
    } else {
        let declaration = line.strip_prefix("path ").unwrap_or("");
        let delimiter = single_colon(declaration)
            .ok_or_else(|| ParseError::new(number, "expected `:` after path product name"))?;
        let (product, template) = declaration.split_at(delimiter);
        (
            Some(qualified_identifier(
                product.trim(),
                number,
                "path product",
            )?),
            &template[1..],
        )
    };
    let template = template.trim();
    let template = template
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            template
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(template)
        .trim();
    if template.is_empty() {
        return Err(ParseError::new(number, "path template must not be empty"));
    }
    if let Some(product) = product {
        if pipeline
            .product_paths
            .insert(product.to_owned(), template.to_owned())
            .is_some()
        {
            return Err(ParseError::new(
                number,
                format!("duplicate path template for product `{product}`"),
            ));
        }
    } else if pipeline
        .path_template
        .replace(template.to_owned())
        .is_some()
    {
        return Err(ParseError::new(number, "duplicate default path template"));
    }
    Ok(())
}

/// As in Bash, an unquoted `#` starts a comment only at the start of a word,
/// so arguments such as `--color=#fff` are kept intact.
pub(crate) fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut word_start = true;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            word_start = false;
            continue;
        }
        match (quote, character) {
            (None, '\\') | (Some('"'), '\\') => escaped = true,
            (None, '\'') => quote = Some('\''),
            (None, '"') => quote = Some('"'),
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (None, '#') if word_start => return &line[..index],
            _ => {}
        }
        word_start = quote.is_none() && character.is_whitespace();
    }
    line
}

fn parse_flow_invocation(
    line: &str,
    number: usize,
    pipeline: &Pipeline,
) -> Result<(Invocation, ProductDef), ParseError> {
    let (left, call) = line
        .split_once('=')
        .ok_or_else(|| ParseError::new(number, "expected flow step: output = operation(inputs)"))?;
    let (output_name, output_type, output_dimensions) = parse_flow_output(left.trim(), number)?;
    let (operation_name, _) = call_parts(call.trim(), number)?;
    pipeline
        .operations
        .iter()
        .find(|operation| operation.name == operation_name)
        .ok_or_else(|| {
            ParseError::new(
                number,
                format!("operation `{operation_name}` must be declared before its first flow step"),
            )
        })?;
    let invocation = parse_invocation_parts(&output_name, call, number)?;
    let dimensions = output_dimensions.unwrap_or_else(|| {
        let input = invocation.inputs.first().map(InputBinding::product_name);
        let input = input.and_then(|name| {
            pipeline
                .products
                .iter()
                .find(|product| product.name == name)
        });
        let mut dimensions = input
            .map(|product| product.dimensions.clone())
            .unwrap_or_default();
        if let Some(InputBinding::Vary { dimension, .. }) = invocation.inputs.first() {
            dimensions.retain(|value| value != dimension);
        }
        dimensions
    });
    Ok((
        invocation,
        ProductDef::new(
            &output_name,
            output_type.unwrap_or(TypeExpr::Unknown),
            &dimensions.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
    ))
}

type FlowOutput = (String, Option<TypeExpr>, Option<Vec<String>>);

fn parse_flow_output(left: &str, number: usize) -> Result<FlowOutput, ParseError> {
    if let Some((name, declaration)) = left.split_once(':') {
        let product = parse_product(&format!("{}:{}", name.trim(), declaration), number)?;
        Ok((
            product.name,
            Some(product.artifact_type),
            Some(product.dimensions),
        ))
    } else {
        Ok((
            identifier(left, number, "output product")?.to_owned(),
            None,
            None,
        ))
    }
}

#[derive(Debug)]
pub(crate) struct UseSpec {
    pub(crate) names: Option<Vec<String>>,
    pub(crate) path: String,
    pub(crate) alias: Option<String>,
}

pub(crate) fn parse_use(line: &str, number: usize) -> Result<UseSpec, ParseError> {
    let syntax = "expected `use path [as alias]` or `use name[, name] from path [as alias]`";
    let rest = line
        .strip_prefix("use ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let (names, path_and_alias) = if rest.starts_with('"') || rest.starts_with('\'') {
        (None, rest)
    } else if let Some((names, path)) = rest.split_once(" from ") {
        let names = comma_items(names, number)?;
        if names.is_empty() {
            return Err(ParseError::new(number, syntax));
        }
        for name in &names {
            qualified_identifier(name, number, "import name")?;
        }
        (Some(names), path)
    } else {
        (None, rest)
    };
    let (path, alias) = parse_use_path(path_and_alias, number)?;
    if path.is_empty() {
        return Err(ParseError::new(number, "import needs a file path"));
    }
    Ok(UseSpec { names, path, alias })
}

fn parse_use_path(text: &str, number: usize) -> Result<(String, Option<String>), ParseError> {
    let text = text.trim();
    if let Some(quote @ ('"' | '\'')) = text.chars().next() {
        let closing = text[1..]
            .find(quote)
            .map(|index| index + 1)
            .ok_or_else(|| ParseError::new(number, "unterminated quoted import path"))?;
        let path = &text[1..closing];
        let tail = text[closing + 1..].trim();
        let alias = if tail.is_empty() {
            None
        } else {
            let value = tail
                .strip_prefix("as ")
                .ok_or_else(|| ParseError::new(number, "expected `as alias` after import path"))?;
            Some(identifier(value.trim(), number, "import alias")?.to_owned())
        };
        Ok((path.to_owned(), alias))
    } else if let Some((path, alias)) = text.rsplit_once(" as ") {
        Ok((
            path.trim().to_owned(),
            Some(identifier(alias.trim(), number, "import alias")?.to_owned()),
        ))
    } else {
        Ok((text.to_owned(), None))
    }
}

/// Parse a text document that may package an inventory alongside its pipeline.
/// The two remain separate values for resolution.
pub fn parse_document(text: &str) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    parse_document_with_imports(text, &BTreeMap::new())
}

pub(crate) fn parse_document_with_imports(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    let mut pipeline_text = String::new();
    let mut inventory_text = String::new();
    let mut inventory_section = false;
    let mut has_inventory = false;

    for original in text.lines() {
        let line = strip_comment(original).trim();
        match line {
            "products:" | "operations:" | "pipeline:" | "constraints:" | "commands:" => {
                inventory_section = false;
            }
            "sources:" | "contexts:" => {
                inventory_section = true;
                has_inventory = true;
            }
            _ if line.starts_with("path:")
                || line.starts_with("path ")
                || line.starts_with("shell-source:")
                || line.starts_with("use ")
                || line.starts_with("source ")
                || line.starts_with("operation ")
                || line.starts_with("command ")
                || line.starts_with("require ") =>
            {
                inventory_section = false;
            }
            _ => {}
        }
        if inventory_section {
            inventory_text.push_str(original);
            pipeline_text.push('\n');
            inventory_text.push('\n');
        } else {
            pipeline_text.push_str(original);
            pipeline_text.push('\n');
            inventory_text.push('\n');
        }
    }

    let pipeline = parse_pipeline_with_imports(&pipeline_text, imports)?;
    let inventory = if has_inventory {
        Some(parse_source_inventory(&inventory_text)?)
    } else {
        None
    };
    Ok((pipeline, inventory))
}

/// Parse an inventory supplied by a dataset indexer or written as a fixture.
/// The inventory contains logical identities, never paths or artifact types.
pub fn parse_source_inventory(text: &str) -> Result<SourceInventory, ParseError> {
    let mut inventory = SourceInventory::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
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
    let product = qualified_identifier(parts.next().unwrap_or(""), number, "constraint product")?;
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
    let dimensions = dimensions.trim();
    let dimensions = dimensions
        .strip_prefix('[')
        .ok_or_else(|| ParseError::new(number, "expected `[` before constraint dimensions"))?;
    let dimensions = dimensions
        .strip_suffix(']')
        .ok_or_else(|| ParseError::new(number, "expected closing `]` in constraint dimensions"))?;
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
    let (declaration, dimensions) = line
        .split_once('[')
        .ok_or_else(|| ParseError::new(number, "expected product name followed by [dimensions]"))?;
    let (name, artifact_type) = if let Some((name, ty)) = declaration.split_once(':') {
        let ty = parse_type_expr(ty.trim(), false)
            .map_err(|message| ParseError::new(number, message))?;
        (name.trim(), ty)
    } else {
        (declaration.trim(), TypeExpr::Unknown)
    };
    let name = identifier(name, number, "product name")?;
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
    let (line, aggregated_dimension) = if let Some((signature, tail)) = line.rsplit_once(" @ drop(")
    {
        let dimension = tail.strip_suffix(')').ok_or_else(|| {
            ParseError::new(
                number,
                "expected `@ drop(dimension)` after operation signature",
            )
        })?;
        (
            signature,
            Some(identifier(
                dimension.trim(),
                number,
                "aggregated dimension",
            )?),
        )
    } else {
        (line, None)
    };
    let (signature, output_type) = if let Some((signature, output_type)) = line.split_once("->") {
        let output_type = parse_type_expr(output_type.trim(), true)
            .map_err(|message| ParseError::new(number, message))?;
        (signature, output_type)
    } else {
        (line, TypeExpr::Unknown)
    };
    let signature = signature.trim();
    if !signature.contains('(') {
        return Err(ParseError::new(number, "expected `(` after operation name"));
    }
    if !line.contains("->")
        && signature
            .split_once(')')
            .is_some_and(|(_, trailing)| !trailing.trim().is_empty())
    {
        return Err(ParseError::new(
            number,
            "expected `->` before operation output type",
        ));
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
    let mut ports = Vec::new();
    let count = inputs.len();
    for (index, input) in inputs.iter().enumerate() {
        let (declared_name, input) = if let Some((name, value)) = input.split_once(':') {
            let name = identifier(name.trim(), number, "input port")?;
            if name == "output" {
                return Err(ParseError::new(
                    number,
                    "input port name `output` is reserved for the operation output",
                ));
            }
            (Some(name), value.trim())
        } else {
            (None, input.as_str())
        };
        let (cardinality, artifact_type) = if input == "many" {
            (Cardinality::Many, TypeExpr::Unknown)
        } else if input == "one" {
            (Cardinality::One, TypeExpr::Unknown)
        } else if let Some(value) = input.strip_prefix("many ") {
            (
                Cardinality::Many,
                parse_type_expr(value.trim(), true)
                    .map_err(|message| ParseError::new(number, message))?,
            )
        } else if let Some(value) = input.strip_prefix("one ") {
            (
                Cardinality::One,
                parse_type_expr(value.trim(), true)
                    .map_err(|message| ParseError::new(number, message))?,
            )
        } else {
            (
                Cardinality::One,
                parse_type_expr(input, true).map_err(|message| ParseError::new(number, message))?,
            )
        };
        let port_name = declared_name.map(str::to_owned).unwrap_or_else(|| {
            if count == 1 {
                "input".to_owned()
            } else {
                format!("input{}", index + 1)
            }
        });
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
    if aggregated_dimension.is_some() && shape_rule != ShapeRule::Aggregate {
        return Err(ParseError::new(
            number,
            "`@ drop(dimension)` requires a many input",
        ));
    }
    let mut operation = OperationDef::new(name, ports, output_type, shape_rule);
    if let Some(dimension) = aggregated_dimension {
        operation = operation.aggregating(dimension);
    }
    Ok(operation)
}

fn parse_invocation(line: &str, number: usize) -> Result<Invocation, ParseError> {
    let (output_product, call) = line
        .split_once('=')
        .ok_or_else(|| ParseError::new(number, "expected `=` in pipeline invocation"))?;
    let output_product = identifier(output_product.trim(), number, "output product")?;
    parse_invocation_parts(output_product, call, number)
}

fn parse_invocation_parts(
    output_product: &str,
    call: &str,
    number: usize,
) -> Result<Invocation, ParseError> {
    let (operation, args) = call_parts(call.trim(), number)?;
    let mut bindings = Vec::new();
    for arg in comma_items(args, number)? {
        if let Some((product, variation)) = arg.split_once('@') {
            let product = qualified_identifier(product.trim(), number, "input product")?;
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
            bindings.push(InputBinding::product(qualified_identifier(
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
    let product = qualified_identifier(product.trim(), number, "source product")?;
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
        .ok_or_else(|| ParseError::new(number, "expected `(` in operation call"))?;
    let name = qualified_identifier(name.trim(), number, "operation name")?;
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
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth
                    .checked_sub(1)
                    .ok_or_else(|| ParseError::new(number, "unexpected `)`"))?
            }
            '<' => angle_depth += 1,
            '>' => {
                angle_depth = angle_depth
                    .checked_sub(1)
                    .ok_or_else(|| ParseError::new(number, "unexpected `>`"))?
            }
            ',' if paren_depth == 0 && angle_depth == 0 => {
                items.push(text[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
    }
    if paren_depth != 0 || angle_depth != 0 {
        return Err(ParseError::new(number, "unclosed `(` or `<`"));
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

fn qualified_identifier<'a>(
    value: &'a str,
    number: usize,
    kind: &str,
) -> Result<&'a str, ParseError> {
    for part in value.split("::") {
        identifier(part, number, kind)?;
    }
    Ok(value)
}
