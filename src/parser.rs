//! Parse sectioned or flow-style pipelines and separate source inventories.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

use crate::bash::check_command_syntax;
use crate::imports::apply_import;
use crate::model::{
    Cardinality, CommandDef, CountRequirement, CoverageRule, EntityBinding, InputBinding,
    InputPort, Invocation, OperationDef, Pipeline, ProductDef, ShapeRule, SourceInventory,
    SourceRecord,
};
use crate::paths::check_path_template_syntax;
use crate::span::{columns_of, content_columns, find_word, Place};
use crate::types::{parse_type_expr, TypeExpr};

const SHELL_SOURCE_REMOVED: &str =
    "shell-source is no longer supported; make the command executable available on PATH";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub line: usize,
    /// The byte range in the line that the error is about, when known.
    pub columns: Option<Range<usize>>,
    pub kind: ParseErrorKind,
    pub message: String,
    /// Where the offending token sits in memory, until `locate` turns it into
    /// columns of the line it was sliced from.
    token: Option<Range<usize>>,
}

/// Errors that callers may want to treat specially; everything else is `Syntax`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseErrorKind {
    Syntax,
    /// A flow step calls an operation that has not been declared yet.
    UndeclaredOperation {
        name: String,
    },
}

impl ParseError {
    pub(crate) fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            columns: None,
            kind: ParseErrorKind::Syntax,
            message: message.into(),
            token: None,
        }
    }

    /// Mark `token`, a slice of the line being parsed, as what the error is about.
    pub(crate) fn at(mut self, token: &str) -> Self {
        let start = token.as_ptr() as usize;
        self.token.get_or_insert(start..start + token.len());
        self
    }

    /// Resolve the marked token to columns of `line`, the text it was sliced
    /// from; without one, point at the line's content.
    pub(crate) fn locate(mut self, line: &str) -> Self {
        if self.columns.is_none() {
            let base = line.as_ptr() as usize;
            let token = self.token.take().and_then(|token| {
                let start = token.start.checked_sub(base)?;
                let end = token.end.checked_sub(base)?;
                (end <= line.len()).then_some(start..end)
            });
            self.columns = Some(token.unwrap_or_else(|| content_columns(line)));
        }
        self
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Where declarations sit in the source, kept beside the parsed [`Pipeline`]
/// so that diagnostics can point at them without the model carrying them.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceMap {
    /// Each product's declared name; a step's output declares its product.
    pub(crate) products: BTreeMap<String, Place>,
    /// Each operation's declared name.
    pub(crate) operations: BTreeMap<String, Place>,
    /// Each step, keyed by the product it produces.
    pub(crate) invocations: BTreeMap<String, Step>,
    /// The last coverage rule for each product.
    pub(crate) constraints: BTreeMap<String, Place>,
    /// One entry per `Pipeline::constraints` element, in the same order.
    pub(crate) rules: Vec<Place>,
    /// One template per `Pipeline::commands` element, in the same order.
    pub(crate) commands: Vec<Place>,
    /// Templates of `path product:` rules, keyed by product.
    pub(crate) paths: BTreeMap<String, Place>,
    /// Template of the default `path:` rule.
    pub(crate) default_path: Option<Place>,
    /// Products and operations brought in by `use` lines.
    pub(crate) imported: BTreeSet<String>,
}

/// Where the parts of one step sit on its line.
#[derive(Clone, Debug, Default)]
pub(crate) struct Step {
    pub(crate) line: usize,
    pub(crate) output: Range<usize>,
    pub(crate) operation: Range<usize>,
    /// From the operation name to the closing parenthesis.
    pub(crate) call: Range<usize>,
    /// One range per input binding, in call order.
    pub(crate) inputs: Vec<Range<usize>>,
}

impl Step {
    fn place(&self, columns: &Range<usize>) -> Place {
        Place::new(self.line, columns.clone())
    }

    pub(crate) fn output(&self) -> Place {
        self.place(&self.output)
    }

    pub(crate) fn operation(&self) -> Place {
        self.place(&self.operation)
    }

    pub(crate) fn call(&self) -> Place {
        self.place(&self.call)
    }

    pub(crate) fn input(&self, index: usize) -> Option<Place> {
        self.inputs.get(index).map(|columns| self.place(columns))
    }
}

impl SourceMap {
    pub(crate) fn command(&self, index: usize) -> Option<Place> {
        self.commands.get(index).cloned()
    }

    /// The template of the path rule that `product` uses.
    pub(crate) fn path_rule(&self, pipeline: &Pipeline, product: &str) -> Option<Place> {
        if pipeline.product_paths.contains_key(product) {
            self.paths.get(product).cloned()
        } else {
            self.default_path.clone()
        }
    }
}

/// A pipeline under construction together with where its declarations sit.
#[derive(Default)]
pub(crate) struct PipelineBuilder {
    pub(crate) pipeline: Pipeline,
    pub(crate) lines: SourceMap,
}

impl PipelineBuilder {
    pub(crate) fn add_product(&mut self, product: ProductDef, place: Place) {
        self.lines.products.insert(product.name.clone(), place);
        self.pipeline.products.push(product);
    }

    pub(crate) fn add_operation(&mut self, operation: OperationDef, place: Place) {
        self.lines.operations.insert(operation.name.clone(), place);
        self.pipeline.operations.push(operation);
    }

    pub(crate) fn add_constraint(&mut self, constraint: CoverageRule, place: Place) {
        self.lines
            .constraints
            .insert(constraint.product.clone(), place.clone());
        self.lines.rules.push(place);
        self.pipeline.constraints.push(constraint);
    }

    pub(crate) fn add_command(&mut self, command: CommandDef, place: Place) {
        self.lines.commands.push(place);
        self.pipeline.commands.push(command);
    }

    fn add_invocation(&mut self, invocation: Invocation, step: Step) {
        self.lines
            .invocations
            .insert(invocation.output_product.clone(), step);
        self.pipeline.invocations.push(invocation);
    }
}

/// Where a parsed declaration's name sits: its first whole-word occurrence
/// at or after `from`, a slice of `original`.
fn name_place(original: &str, number: usize, from: &str, name: &str) -> Place {
    let start = columns_of(original, from).map_or(0, |columns| columns.start);
    Place::new(
        number,
        find_word(original, start, name).unwrap_or_else(|| content_columns(original)),
    )
}

/// Where a parsed step's output, operation, call, and inputs sit on its line.
fn step_place(original: &str, number: usize, invocation: &Invocation) -> Step {
    let content = content_columns(original);
    let found = |from: usize, word: &str| find_word(original, from, word);
    let output = found(content.start, &invocation.output_product).unwrap_or(content.clone());
    let equals = original[output.end..]
        .find('=')
        .map_or(output.end, |offset| output.end + offset + 1);
    let operation = found(equals, &invocation.operation).unwrap_or(content.clone());
    let call_end = original[..content.end]
        .rfind(')')
        .map_or(content.end, |index| index + 1);
    let mut from = operation.end;
    let inputs = invocation
        .inputs
        .iter()
        .map(|binding| {
            let Some(product) = found(from, binding.product_name()) else {
                return operation.start..call_end;
            };
            // A `vary` binding runs to the parenthesis that closes it.
            let end = match binding {
                InputBinding::Product(_) => product.end,
                InputBinding::Vary { .. } => original[product.end..call_end]
                    .find(')')
                    .map_or(product.end, |offset| product.end + offset + 1),
            };
            from = end;
            product.start..end
        })
        .collect();
    Step {
        line: number,
        output,
        call: operation.start..call_end,
        operation,
        inputs,
    }
}

/// Where the text at the end of a declaration sits, such as a template.
fn tail_place(original: &str, number: usize, tail: &str) -> Place {
    let content = content_columns(original);
    let columns = original[content.clone()]
        .rfind(tail)
        .map_or(content.clone(), |offset| {
            content.start + offset..content.start + offset + tail.len()
        });
    Place::new(number, columns)
}

/// A parsed document: its pipeline, any inline inventory, and declaration lines.
pub(crate) struct ParsedDocument {
    pub(crate) pipeline: Pipeline,
    pub(crate) inventory: Option<SourceInventory>,
    pub(crate) lines: SourceMap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Section {
    Products,
    Operations,
    Pipeline,
    Constraints,
    Commands,
}

pub fn parse_pipeline(text: &str) -> Result<Pipeline, ParseError> {
    parse_pipeline_with_imports(text, &BTreeMap::new()).map(|builder| builder.pipeline)
}

fn parse_pipeline_with_imports(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
) -> Result<PipelineBuilder, ParseError> {
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
) -> Result<PipelineBuilder, ParseError> {
    let mut builder = PipelineBuilder::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        sectioned_line(&mut builder, &mut section, imports, original, index + 1)
            .map_err(|error| error.locate(original))?;
    }
    Ok(builder)
}

fn sectioned_line(
    builder: &mut PipelineBuilder,
    section: &mut Option<Section>,
    imports: &BTreeMap<usize, Pipeline>,
    original: &str,
    number: usize,
) -> Result<(), ParseError> {
    let line = strip_comment(original).trim();
    match line {
        "" => {}
        "products:" => *section = Some(Section::Products),
        "operations:" => *section = Some(Section::Operations),
        "pipeline:" => *section = Some(Section::Pipeline),
        "constraints:" => *section = Some(Section::Constraints),
        "commands:" => *section = Some(Section::Commands),
        source if source.starts_with("use ") => {
            apply_import(builder, imports, Place::new(number, content_columns(original)))?;
            *section = None;
        }
        source if source.starts_with("shell-source:") => {
            return Err(ParseError::new(number, SHELL_SOURCE_REMOVED));
        }
        path if path.starts_with("path:") || path.starts_with("path ") => {
            set_path(builder, original, path, number)?;
            *section = None;
        }
        "sources:" | "contexts:" => {
            return Err(ParseError::new(
                number,
                "source inventory is separate from Pipeline; use parse_document for a combined text file",
            ))
        }
        _ => match section {
            Some(Section::Products) => {
                let product = parse_product(line, number)?;
                let place = name_place(original, number, line, &product.name);
                builder.add_product(product, place);
            }
            Some(Section::Operations) => {
                let operation = parse_operation(line, number)?;
                let place = name_place(original, number, line, &operation.name);
                builder.add_operation(operation, place);
            }
            Some(Section::Pipeline) => {
                let invocation = parse_invocation(line, number)?;
                let step = step_place(original, number, &invocation);
                builder.add_invocation(invocation, step);
            }
            Some(Section::Constraints) => {
                let rule = parse_coverage_rule(line, number)?;
                builder.add_constraint(rule, Place::new(number, content_columns(original)));
            }
            Some(Section::Commands) => {
                let command = parse_command(line, number)?;
                let place = tail_place(original, number, &command.template);
                builder.add_command(command, place);
            }
            None => {
                return Err(ParseError::new(
                    number,
                    "expected a section header: products:, operations:, pipeline:, constraints:, or commands:",
                ))
            }
        },
    }
    Ok(())
}

fn parse_flow_pipeline(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
) -> Result<PipelineBuilder, ParseError> {
    let mut builder = PipelineBuilder::default();
    for (index, original) in text.lines().enumerate() {
        flow_line(&mut builder, imports, original, index + 1)
            .map_err(|error| error.locate(original))?;
    }
    Ok(builder)
}

fn flow_line(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    original: &str,
    number: usize,
) -> Result<(), ParseError> {
    let line = strip_comment(original).trim();
    if line.is_empty() {
        return Ok(());
    }
    if line.starts_with("use ") {
        apply_import(
            builder,
            imports,
            Place::new(number, content_columns(original)),
        )?;
    } else if let Some(declaration) = line.strip_prefix("source ") {
        let declaration = declaration.trim();
        let product = parse_product(declaration, number)?;
        let place = name_place(original, number, declaration, &product.name);
        builder.add_product(product, place);
    } else if let Some(declaration) = line.strip_prefix("operation ") {
        let declaration = declaration.trim();
        let operation = parse_operation(declaration, number)?;
        let place = name_place(original, number, declaration, &operation.name);
        builder.add_operation(operation, place);
    } else if line.starts_with("require ") {
        let rule = parse_coverage_rule(line, number)?;
        builder.add_constraint(rule, Place::new(number, content_columns(original)));
    } else if let Some(declaration) = line.strip_prefix("command ") {
        let command = parse_command(declaration.trim(), number)?;
        let place = tail_place(original, number, &command.template);
        builder.add_command(command, place);
    } else if line.starts_with("shell-source:") {
        return Err(ParseError::new(number, SHELL_SOURCE_REMOVED));
    } else if line.starts_with("path ") || line.starts_with("path:") {
        set_path(builder, original, line, number)?;
    } else if line.contains('=') {
        let (invocation, product) = parse_flow_invocation(line, number, &builder.pipeline)?;
        let step = step_place(original, number, &invocation);
        builder.add_product(product, step.output());
        builder.add_invocation(invocation, step);
    } else if line.contains('(') && line.ends_with(')') {
        return Err(ParseError::new(
            number,
            "expected `=` before operation call",
        ));
    } else {
        return Err(ParseError::new(
            number,
            "expected source, operation, require, or output = operation(inputs)",
        ));
    }
    Ok(())
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
    check_command_syntax(template).map_err(|error| {
        ParseError::new(number, format!("command `{operation}`: {}", error.message)).at(template)
    })?;
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

fn set_path(
    builder: &mut PipelineBuilder,
    original: &str,
    line: &str,
    number: usize,
) -> Result<(), ParseError> {
    let PipelineBuilder { pipeline, lines } = builder;
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
    check_path_template_syntax(template)
        .map_err(|error| ParseError::new(number, error.message).at(template))?;
    if let Some(product) = product {
        lines
            .paths
            .insert(product.to_owned(), tail_place(original, number, template));
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
    } else {
        lines.default_path = Some(tail_place(original, number, template));
    }
    Ok(())
}

/// As in Bash, an unquoted `#` starts a comment only at the start of a word,
/// so arguments such as `--color=#fff` are kept intact.
pub(crate) fn strip_comment(line: &str) -> &str {
    &line[..comment_start(line).unwrap_or(line.len())]
}

fn comment_start(line: &str) -> Option<usize> {
    scan_hashes(line).find_map(|hash| hash.starts_word.then_some(hash.index))
}

/// The word before an unquoted `#` that ends it, as in `word# note`: the `#`
/// stays part of the word, though it reads like the start of a comment.
pub(crate) fn glued_comment(line: &str) -> Option<&str> {
    let end = comment_start(line).unwrap_or(line.len());
    scan_hashes(&line[..end]).find_map(|hash| {
        let ends_word = line[hash.index + 1..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace);
        (!hash.starts_word && ends_word).then(|| {
            let word_start = line[..hash.index]
                .rfind(char::is_whitespace)
                .map_or(0, |index| index + 1);
            &line[word_start..hash.index]
        })
    })
}

struct Hash {
    index: usize,
    starts_word: bool,
}

/// Every unquoted, unescaped `#` in `line`.
fn scan_hashes(line: &str) -> impl Iterator<Item = Hash> + '_ {
    let mut quote = None;
    let mut escaped = false;
    let mut word_start = true;
    line.char_indices().filter_map(move |(index, character)| {
        let at_word_start = word_start;
        word_start = false;
        if escaped {
            escaped = false;
            return None;
        }
        match (quote, character) {
            (None | Some('"'), '\\') => escaped = true,
            (None, '\'') => quote = Some('\''),
            (None, '"') => quote = Some('"'),
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (None, '#') => {
                return Some(Hash {
                    index,
                    starts_word: at_word_start,
                })
            }
            _ => {}
        }
        word_start = quote.is_none() && character.is_whitespace();
        None
    })
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
            let mut error = ParseError::new(
                number,
                format!("operation `{operation_name}` must be declared before its first flow step"),
            )
            .at(operation_name);
            error.kind = ParseErrorKind::UndeclaredOperation {
                name: operation_name.to_owned(),
            };
            error
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
            &dimensions,
        ),
    ))
}

type FlowOutput = (String, Option<TypeExpr>, Option<Vec<String>>);

fn parse_flow_output(left: &str, number: usize) -> Result<FlowOutput, ParseError> {
    if left.contains(':') {
        let product = parse_product(left, number)?;
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
        (Some(names.into_iter().map(str::to_owned).collect()), path)
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
        .map(|document| (document.pipeline, document.inventory))
}

pub(crate) fn parse_document_with_imports(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
) -> Result<ParsedDocument, ParseError> {
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
        // Each line goes to one side and a blank line to the other, so both
        // texts keep the document's line numbers.
        let target = if inventory_section {
            &mut inventory_text
        } else {
            &mut pipeline_text
        };
        target.push_str(original);
        pipeline_text.push('\n');
        inventory_text.push('\n');
    }

    let builder = parse_pipeline_with_imports(&pipeline_text, imports)?;
    let inventory = has_inventory
        .then(|| parse_source_inventory(&inventory_text))
        .transpose()?;
    Ok(ParsedDocument {
        pipeline: builder.pipeline,
        inventory,
        lines: builder.lines,
    })
}

/// Parse an inventory supplied by a dataset indexer or written as a fixture.
/// The inventory contains logical identities, never paths or artifact types.
pub fn parse_source_inventory(text: &str) -> Result<SourceInventory, ParseError> {
    enum InventorySection {
        Sources,
        Contexts,
    }

    let mut inventory = SourceInventory::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "sources:" => section = Some(InventorySection::Sources),
            "contexts:" => section = Some(InventorySection::Contexts),
            _ => match section {
                Some(InventorySection::Sources) => {
                    let record = parse_source(line, number).map_err(|e| e.locate(original))?;
                    inventory.artifacts.push(record);
                }
                Some(InventorySection::Contexts) => {
                    let context = parse_context(line, number).map_err(|e| e.locate(original))?;
                    inventory.contexts.push(context);
                }
                None => {
                    return Err(ParseError::new(
                        number,
                        "expected inventory section header: sources: or contexts:",
                    )
                    .locate(original))
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
    Ok(CoverageRule::new(product, &dimensions, count))
}

fn parse_count(value: &str, number: usize) -> Result<usize, ParseError> {
    value.parse().map_err(|_| {
        ParseError::new(number, "constraint count must be a nonnegative integer").at(value)
    })
}

fn parse_product(line: &str, number: usize) -> Result<ProductDef, ParseError> {
    let (declaration, dimensions) = line
        .split_once('[')
        .ok_or_else(|| ParseError::new(number, "expected product name followed by [dimensions]"))?;
    let (name, artifact_type) = if let Some((name, ty)) = declaration.split_once(':') {
        let ty = ty.trim();
        let ty = parse_type_expr(ty, false)
            .map_err(|error| ParseError::new(number, error.message).at(ty))?;
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
    Ok(ProductDef::new(name, artifact_type, &dimensions))
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
        let output_type = output_type.trim();
        let output_type = parse_type_expr(output_type, true)
            .map_err(|error| ParseError::new(number, error.message).at(output_type))?;
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
                )
                .at(name));
            }
            (Some(name), value.trim())
        } else {
            (None, *input)
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

fn port_type(text: &str, number: usize) -> Result<TypeExpr, ParseError> {
    parse_type_expr(text, true).map_err(|error| ParseError::new(number, error.message).at(text))
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
                return Err(
                    ParseError::new(number, "only `@ vary(dimension)` is supported").at(keyword),
                );
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
            )
            .at(item));
        }
        if values
            .insert(dimension.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(ParseError::new(
                number,
                format!("duplicate source dimension `{dimension}`"),
            )
            .at(item));
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

fn comma_items(text: &str, number: usize) -> Result<Vec<&str>, ParseError> {
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
                items.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if paren_depth != 0 || angle_depth != 0 {
        return Err(ParseError::new(number, "unclosed `(` or `<`"));
    }
    items.push(text[start..].trim());
    if items.iter().any(|item| item.is_empty()) {
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
        )
        .at(value));
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
