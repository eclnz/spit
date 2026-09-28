//! Parse sectioned or flow-style pipelines and separate source inventories.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

use crate::command::CommandTemplate;
use crate::model::{
    Cardinality, CommandDef, CommandRole, CountRequirement, CoverageRule, DefaultPort,
    EntityBinding, InputBinding, InputPort, Invocation, OperationDef, OutputPort, Pipeline,
    ProductDef, ShapeRule, SourceInventory, SourceRecord, DEFAULT_OUTPUT,
};
use crate::paths::PathTemplate;
use crate::span::{columns_of, content_columns, find_word, Focus, Located, Place};
use crate::types::{parse_type_expr, TypeExpr, TypeParseError};

const SHELL_SOURCE_REMOVED: &str =
    "shell-source is no longer supported; make the command executable available on PATH";

/// A parse error, with the line it is on.
pub type ParseError = Located<ParseFailure>;

/// What went wrong while parsing a line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseFailure {
    pub kind: ParseErrorKind,
    pub message: String,
}

impl fmt::Display for ParseFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
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
        let mut error = Located::unplaced(ParseFailure {
            kind: ParseErrorKind::Syntax,
            message: message.into(),
        });
        error.location.line = Some(line);
        error
    }

    /// The line the error is on; every parse error has one.
    pub fn line(&self) -> usize {
        self.location.line.unwrap_or_default()
    }

    /// Mark `token`, a slice of the line being parsed, as what the error is about.
    pub(crate) fn at_token(mut self, token: &str) -> Self {
        let start = token.as_ptr() as usize;
        self.location
            .focus
            .get_or_insert(Focus::Slice(start..start + token.len()));
        self
    }

    /// Point at `place` unless the error already points somewhere, for an
    /// error found after its line was parsed.
    pub(crate) fn within(mut self, place: &Place) -> Self {
        if self.location.columns.is_none() && self.location.focus.is_none() {
            self.location.columns = Some(place.columns.clone());
        }
        self
    }

    /// Resolve the marked token to columns of `line`, the text it was sliced
    /// from; without one, point at the line's content.
    pub(crate) fn locate(mut self, line: &str) -> Self {
        if self.location.columns.is_none() {
            let base = line.as_ptr() as usize;
            let token = match self.location.focus.take() {
                Some(Focus::Slice(token)) => Some(token),
                _ => None,
            };
            let token = token.and_then(|token| {
                let start = token.start.checked_sub(base)?;
                let end = token.end.checked_sub(base)?;
                (end <= line.len()).then_some(start..end)
            });
            self.location.columns = Some(token.unwrap_or_else(|| content_columns(line)));
        }
        self
    }
}

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
    pub(crate) constraints: BTreeMap<String, Rule>,
    /// One entry per `Pipeline::constraints` element, in the same order.
    pub(crate) rules: Vec<Rule>,
    /// One template per `Pipeline::commands` element, in the same order.
    pub(crate) commands: Vec<Place>,
    /// Templates of `path product:` rules, keyed by product.
    pub(crate) paths: BTreeMap<String, Place>,
    /// Template of the default `path:` rule.
    pub(crate) default_path: Option<Place>,
    /// Each stage's name in its `stage` header.
    pub(crate) stages: BTreeMap<String, Place>,
    /// Templates of the `path:` rules inside stages, keyed by stage.
    pub(crate) stage_paths: BTreeMap<String, Place>,
    /// Products and operations brought in by `use` lines.
    pub(crate) imported: BTreeSet<String>,
}

/// Where the parts of one step sit on its line.
#[derive(Clone, Debug, Default)]
pub(crate) struct Step {
    pub(crate) line: usize,
    /// One range per output product, in call order.
    pub(crate) outputs: Vec<Range<usize>>,
    pub(crate) operation: Range<usize>,
    /// From the operation name to the closing parenthesis.
    pub(crate) call: Range<usize>,
    /// One range per input binding, with its selectors, in call order.
    pub(crate) inputs: Vec<Range<usize>>,
}

impl Step {
    fn place(&self, columns: &Range<usize>) -> Place {
        Place::new(self.line, columns.clone())
    }

    /// The first output product.
    pub(crate) fn output(&self) -> Place {
        self.output_at(0)
    }

    pub(crate) fn output_at(&self, index: usize) -> Place {
        let columns = self.outputs.get(index).or(self.outputs.first());
        columns.map_or_else(|| self.call(), |columns| self.place(columns))
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

/// Where the parts of one coverage rule sit on its line.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rule {
    pub(crate) line: usize,
    pub(crate) whole: Range<usize>,
    pub(crate) product: Range<usize>,
    /// The bracketed dimensions the rule groups by.
    pub(crate) dimensions: Range<usize>,
}

impl Rule {
    /// A rule known only as a whole, such as one brought in by an import.
    pub(crate) fn spanning(place: &Place) -> Self {
        Self {
            line: place.line,
            whole: place.columns.clone(),
            product: place.columns.clone(),
            dimensions: place.columns.clone(),
        }
    }

    pub(crate) fn whole(&self) -> Place {
        Place::new(self.line, self.whole.clone())
    }

    pub(crate) fn product(&self) -> Place {
        Place::new(self.line, self.product.clone())
    }

    pub(crate) fn dimensions(&self) -> Place {
        Place::new(self.line, self.dimensions.clone())
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
        } else if let Some((stage, _)) = pipeline.stage_path_rule(product) {
            self.stage_paths.get(stage).cloned()
        } else {
            self.default_path.clone()
        }
    }
}

/// A pipeline as written: its statements in source order, before lowering
/// turns them into a [`Pipeline`]. Each keeps where its parts sit.
#[derive(Clone, Debug, Default)]
pub(crate) struct Syntax {
    pub(crate) statements: Vec<Statement>,
}

#[derive(Clone, Debug)]
pub(crate) struct Statement {
    /// The statement's line content, for errors about it as a whole.
    pub(crate) place: Place,
    pub(crate) kind: StatementKind,
}

#[derive(Clone, Debug)]
pub(crate) enum StatementKind {
    /// A `use` line; the definitions it brings in are loaded separately.
    Import,
    /// A `stage name:` header, by the stage's full name.
    Stage {
        name: String,
        place: Place,
    },
    /// A `source` declaration or an entry of a `products:` section.
    Product(ProductDef, Place),
    Operation(OperationDef, Place),
    Constraint(CoverageRule, Rule),
    Command(CommandDef, Place),
    Path(PathRule),
    /// A step whose outputs are declared elsewhere, from a `pipeline:` section.
    Step(Invocation, Step),
    /// A flow step, which declares its output products.
    FlowStep(FlowStep),
}

/// A `path:` or `path product:` rule.
#[derive(Clone, Debug)]
pub(crate) struct PathRule {
    /// The product it is for; without one, the default for `stage` or,
    /// outside every stage, for the whole pipeline.
    pub(crate) product: Option<String>,
    pub(crate) stage: Option<String>,
    pub(crate) template: PathTemplate,
    /// Where the template sits.
    pub(crate) place: Place,
}

/// A step written `outputs = operation(inputs)`. Each output declares a
/// product, whose type and dimensions may be left for lowering to infer.
#[derive(Clone, Debug)]
pub(crate) struct FlowStep {
    pub(crate) invocation: Invocation,
    pub(crate) outputs: Vec<FlowOutput>,
    pub(crate) step: Step,
}

#[derive(Clone, Debug)]
pub(crate) struct FlowOutput {
    pub(crate) name: String,
    pub(crate) artifact_type: Option<TypeExpr>,
    pub(crate) dimensions: Option<Vec<String>>,
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

/// Where a parsed step's outputs, operation, call, and inputs sit on its line.
fn step_place(original: &str, number: usize, invocation: &Invocation) -> Step {
    let content = content_columns(original);
    let found = |from: usize, word: &str| find_word(original, from, word);
    let mut from = content.start;
    let outputs: Vec<_> = invocation
        .outputs
        .iter()
        .map(|output| {
            let columns = found(from, output).unwrap_or(content.clone());
            from = columns.end;
            columns
        })
        .collect();
    let equals = original[from..]
        .find('=')
        .map_or(from, |offset| from + offset + 1);
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
            // Selectors run to the end of the argument.
            let end = if binding.has_selectors() {
                argument_end(original, product.end, call_end)
            } else {
                product.end
            };
            from = end;
            product.start..end
        })
        .collect();
    Step {
        line: number,
        outputs,
        call: operation.start..call_end,
        operation,
        inputs,
    }
}

/// The end of the call argument that continues at `start`: the next
/// top-level `,` or the call's closing parenthesis, less trailing space.
fn argument_end(line: &str, start: usize, call_end: usize) -> usize {
    let mut depth = 0usize;
    let mut end = call_end.saturating_sub(1).max(start);
    for (offset, character) in line[start..call_end].char_indices() {
        match character {
            '(' => depth += 1,
            ')' if depth == 0 => {
                end = start + offset;
                break;
            }
            ')' => depth -= 1,
            ',' if depth == 0 => {
                end = start + offset;
                break;
            }
            _ => {}
        }
    }
    start + line[start..end].trim_end().len()
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

/// Where a parsed coverage rule's product and grouped dimensions sit.
fn rule_place(original: &str, number: usize, rule: &CoverageRule) -> Rule {
    let whole = content_columns(original);
    let after_keyword = whole.start + "require".len();
    let product = find_word(original, after_keyword, &rule.product).unwrap_or(whole.clone());
    let dimensions = original[product.end..whole.end]
        .find('[')
        .map_or(whole.clone(), |offset| product.end + offset..whole.end);
    Rule {
        line: number,
        whole,
        product,
        dimensions,
    }
}

/// Whether to read a document's inline inventory. A separate inventory
/// replaces it, so it is then skipped rather than required to parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InlineInventory {
    Read,
    Skip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Section {
    Products,
    Operations,
    Pipeline,
    Constraints,
    Commands,
}

/// Parse a pipeline's statements, in the sectioned or the flow form.
pub(crate) fn parse_syntax(text: &str) -> Result<Syntax, ParseError> {
    let mut syntax = Syntax::default();
    if is_sectioned_document(text) {
        let mut section = None;
        for (index, original) in text.lines().enumerate() {
            sectioned_line(&mut syntax, &mut section, original, index + 1)
                .map_err(|error| error.locate(original))?;
        }
    } else {
        let mut stages = OpenStages::default();
        for (index, original) in text.lines().enumerate() {
            flow_line(&mut syntax, &mut stages, original, index + 1)
                .map_err(|error| error.locate(original))?;
        }
    }
    Ok(syntax)
}

impl Syntax {
    fn push(&mut self, original: &str, number: usize, kind: StatementKind) {
        self.statements.push(Statement {
            place: Place::new(number, content_columns(original)),
            kind,
        });
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

fn sectioned_line(
    syntax: &mut Syntax,
    section: &mut Option<Section>,
    original: &str,
    number: usize,
) -> Result<(), ParseError> {
    let line = strip_comment(original).trim();
    let mut push = |kind| syntax.push(original, number, kind);
    match line {
        "" => {}
        "products:" => *section = Some(Section::Products),
        "operations:" => *section = Some(Section::Operations),
        "pipeline:" => *section = Some(Section::Pipeline),
        "constraints:" => *section = Some(Section::Constraints),
        "commands:" => *section = Some(Section::Commands),
        source if source.starts_with("use ") => {
            push(StatementKind::Import);
            *section = None;
        }
        source if source.starts_with("shell-source:") => {
            return Err(ParseError::new(number, SHELL_SOURCE_REMOVED));
        }
        stage if is_stage_header(stage) => {
            return Err(ParseError::new(
                number,
                "stages are written in the flow form, not in a sectioned document",
            ));
        }
        path if path.starts_with("path:") || path.starts_with("path ") => {
            push(StatementKind::Path(parse_path(None, original, path, number)?));
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
                push(StatementKind::Product(product, place));
            }
            Some(Section::Operations) => {
                let operation = parse_operation(line, number)?;
                let place = name_place(original, number, line, &operation.name);
                push(StatementKind::Operation(operation, place));
            }
            Some(Section::Pipeline) => {
                let invocation = parse_invocation(line, number)?;
                let step = step_place(original, number, &invocation);
                push(StatementKind::Step(invocation, step));
            }
            Some(Section::Constraints) => {
                let rule = parse_coverage_rule(line, number)?;
                let place = rule_place(original, number, &rule);
                push(StatementKind::Constraint(rule, place));
            }
            Some(Section::Commands) => {
                let command = match line.strip_prefix("verify ") {
                    Some(declaration) => {
                        parse_command(declaration.trim(), number, CommandRole::Verify)?
                    }
                    None => parse_command(line, number, CommandRole::Run)?,
                };
                let place = tail_place(original, number, command.template.as_str());
                push(StatementKind::Command(command, place));
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

/// Whether a line opens a stage, as opposed to a step whose output product
/// happens to be called `stage`.
fn is_stage_header(line: &str) -> bool {
    line.starts_with("stage ") && !line.contains('=')
}

/// The stages open at a line of the flow form, outermost first.
#[derive(Default)]
struct OpenStages(Vec<OpenStage>);

struct OpenStage {
    /// The full name, such as `preprocess/denoise`.
    name: String,
    /// The indentation of the stage's header.
    header: usize,
    /// The indentation the stage's lines share, once the first is read.
    body: Option<usize>,
}

impl OpenStages {
    /// The innermost open stage.
    fn current(&self) -> Option<&str> {
        self.0.last().map(|stage| stage.name.as_str())
    }

    /// Close each stage that a line indented by `indent` is not inside, then
    /// check that the line lines up with the other lines of its stage.
    fn enter(&mut self, indent: usize, number: usize) -> Result<(), ParseError> {
        while self.0.last().is_some_and(|stage| indent <= stage.header) {
            self.0.pop();
        }
        let Some(stage) = self.0.last_mut() else {
            return Ok(());
        };
        match stage.body {
            None => stage.body = Some(indent),
            Some(body) if body == indent => {}
            Some(_) => {
                return Err(ParseError::new(
                    number,
                    format!(
                        "this line is indented differently from the other lines of stage `{}`",
                        stage.name
                    ),
                ))
            }
        }
        Ok(())
    }
}

/// Open a stage for a `stage name:` header, inside the current stage if
/// there is one. Its lines are indented beneath it, and the next line that
/// is not ends it.
fn open_stage(
    syntax: &mut Syntax,
    stages: &mut OpenStages,
    original: &str,
    declaration: &str,
    indent: usize,
    number: usize,
) -> Result<(), ParseError> {
    let syntax_error = "expected `stage name:`, with the stage's lines indented beneath it";
    if stages.current().is_none() && indent > 0 {
        return Err(ParseError::new(
            number,
            "a stage header outside every stage starts at the beginning of its line",
        ));
    }
    let name = declaration
        .trim()
        .strip_suffix(':')
        .ok_or_else(|| ParseError::new(number, syntax_error))?;
    let name = identifier(name.trim(), number, "stage name")?;
    let full = match stages.current() {
        Some(parent) => format!("{parent}/{name}"),
        None => name.to_owned(),
    };
    let place = name_place(original, number, declaration, name);
    syntax.push(
        original,
        number,
        StatementKind::Stage {
            name: full.clone(),
            place,
        },
    );
    stages.0.push(OpenStage {
        name: full,
        header: indent,
        body: None,
    });
    Ok(())
}

fn flow_line(
    syntax: &mut Syntax,
    stages: &mut OpenStages,
    original: &str,
    number: usize,
) -> Result<(), ParseError> {
    let line = strip_comment(original).trim();
    if line.is_empty() {
        return Ok(());
    }
    let indent = original.len() - original.trim_start().len();
    stages.enter(indent, number)?;
    let stage = stages.current().map(str::to_owned);
    let top_level_only = |what: &str| {
        stage.as_ref().map_or(Ok(()), |name| {
            Err(ParseError::new(
                number,
                format!("{what} belongs at the top level, outside stage `{name}`"),
            ))
        })
    };
    let kind = if let Some(declaration) = line
        .strip_prefix("stage ")
        .filter(|_| is_stage_header(line))
    {
        return open_stage(syntax, stages, original, declaration, indent, number);
    } else if line.starts_with("use ") {
        top_level_only("`use`")?;
        StatementKind::Import
    } else if let Some(declaration) = line.strip_prefix("source ") {
        top_level_only("`source`, which declares an input,")?;
        let declaration = declaration.trim();
        let product = parse_product(declaration, number)?;
        let place = name_place(original, number, declaration, &product.name);
        StatementKind::Product(product, place)
    } else if let Some(declaration) = line.strip_prefix("operation ") {
        let declaration = declaration.trim();
        let operation = parse_operation(declaration, number)?;
        let place = name_place(original, number, declaration, &operation.name);
        StatementKind::Operation(operation, place)
    } else if line.starts_with("require ") {
        top_level_only("`require`, which checks sources,")?;
        let rule = parse_coverage_rule(line, number)?;
        let place = rule_place(original, number, &rule);
        StatementKind::Constraint(rule, place)
    } else if let Some(declaration) = line.strip_prefix("command ") {
        let command = parse_command(declaration.trim(), number, CommandRole::Run)?;
        let place = tail_place(original, number, command.template.as_str());
        StatementKind::Command(command, place)
    } else if let Some(declaration) = line.strip_prefix("verify ") {
        let command = parse_command(declaration.trim(), number, CommandRole::Verify)?;
        let place = tail_place(original, number, command.template.as_str());
        StatementKind::Command(command, place)
    } else if line.starts_with("shell-source:") {
        return Err(ParseError::new(number, SHELL_SOURCE_REMOVED));
    } else if line.starts_with("path ") || line.starts_with("path:") {
        StatementKind::Path(parse_path(stage, original, line, number)?)
    } else if line.contains('=') {
        let (mut invocation, outputs) = parse_flow_step(line, number)?;
        invocation.stage.clone_from(&stage);
        let step = step_place(original, number, &invocation);
        StatementKind::FlowStep(FlowStep {
            invocation,
            outputs,
            step,
        })
    } else if line.contains('(') && line.ends_with(')') {
        return Err(ParseError::new(
            number,
            "expected `=` before operation call",
        ));
    } else {
        return Err(ParseError::new(
            number,
            "expected source, operation, command, verify, require, path, stage, or output = operation(inputs)",
        ));
    };
    syntax.push(original, number, kind);
    Ok(())
}

fn parse_command(line: &str, number: usize, role: CommandRole) -> Result<CommandDef, ParseError> {
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
    let template = rest[1..].trim();
    if template.is_empty() {
        return Err(ParseError::new(
            number,
            "command template must not be empty",
        ));
    }
    let parsed = CommandTemplate::parse(template).map_err(|error| {
        ParseError::new(
            number,
            format!("command `{operation}`: {}", error.message()),
        )
        .at_token(template)
    })?;
    Ok(CommandDef {
        role,
        ..CommandDef::new(operation, parsed)
    })
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

/// Parse a path rule. A default `path:` inside `stage` covers only that
/// stage's products.
fn parse_path(
    stage: Option<String>,
    original: &str,
    line: &str,
    number: usize,
) -> Result<PathRule, ParseError> {
    let (product, template) = if let Some(template) = line.strip_prefix("path:") {
        (None, template)
    } else {
        let declaration = line.strip_prefix("path ").unwrap_or("");
        let delimiter = single_colon(declaration)
            .ok_or_else(|| ParseError::new(number, "expected `:` after path product name"))?;
        let (product, template) = declaration.split_at(delimiter);
        (
            Some(qualified_identifier(product.trim(), number, "path product")?.to_owned()),
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
    let parsed = PathTemplate::parse(template)
        .map_err(|error| ParseError::new(number, error.message()).at_token(template))?;
    Ok(PathRule {
        stage: stage.filter(|_| product.is_none()),
        product,
        template: parsed,
        place: tail_place(original, number, template),
    })
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

/// Parse `outputs = operation(inputs)`, where each output may declare its
/// product's type and dimensions.
fn parse_flow_step(line: &str, number: usize) -> Result<(Invocation, Vec<FlowOutput>), ParseError> {
    let (left, call) = line
        .split_once('=')
        .ok_or_else(|| ParseError::new(number, "expected flow step: output = operation(inputs)"))?;
    let outputs = comma_items(left, number)?
        .into_iter()
        .map(|output| parse_flow_output(output, number))
        .collect::<Result<Vec<_>, _>>()?;
    if outputs.is_empty() {
        return Err(ParseError::new(
            number,
            "expected an output product before `=`",
        ));
    }
    let names = outputs.iter().map(|output| output.name.clone()).collect();
    let invocation = parse_invocation_parts(names, call, number)?;
    Ok((invocation, outputs))
}

fn parse_flow_output(left: &str, number: usize) -> Result<FlowOutput, ParseError> {
    if left.contains(':') {
        let product = parse_product(left, number)?;
        Ok(FlowOutput {
            name: product.name,
            artifact_type: Some(product.artifact_type),
            dimensions: Some(product.dimensions),
        })
    } else {
        Ok(FlowOutput {
            name: identifier(left, number, "output product")?.to_owned(),
            artifact_type: None,
            dimensions: None,
        })
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

/// A document's text split in two: the pipeline, and any inline inventory
/// under `sources:` or `contexts:` headers. Each keeps the document's line
/// numbers, with blank lines where the other's lines were.
pub(crate) struct DocumentText {
    pub(crate) pipeline: String,
    pub(crate) inventory: String,
    /// The line of the first inline `sources:` or `contexts:` header.
    pub(crate) inventory_line: Option<usize>,
}

pub(crate) fn split_document(text: &str) -> DocumentText {
    let mut pipeline_text = String::new();
    let mut inventory_text = String::new();
    let mut inventory_section = false;
    let mut inventory_line = None;

    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        match line {
            "products:" | "operations:" | "pipeline:" | "constraints:" | "commands:" => {
                inventory_section = false;
            }
            "sources:" | "contexts:" => {
                inventory_section = true;
                inventory_line.get_or_insert(index + 1);
            }
            _ if line.starts_with("path:")
                || line.starts_with("path ")
                || line.starts_with("shell-source:")
                || line.starts_with("use ")
                || line.starts_with("source ")
                || line.starts_with("operation ")
                || line.starts_with("command ")
                || line.starts_with("verify ")
                || line.starts_with("require ")
                || is_stage_header(line) =>
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
    DocumentText {
        pipeline: pipeline_text,
        inventory: inventory_text,
        inventory_line,
    }
}

/// Write an inventory in the text form [`parse_source_inventory`] reads,
/// with each record's values in its product's declared dimension order.
pub fn render_source_inventory(inventory: &SourceInventory, pipeline: &Pipeline) -> String {
    let mut text = String::new();
    if !inventory.contexts.is_empty() {
        text.push_str("contexts:\n");
        for context in &inventory.contexts {
            text.push_str(&format!("    [{context}]\n"));
        }
    }
    text.push_str("sources:\n");
    for record in &inventory.artifacts {
        let declared = pipeline
            .products
            .iter()
            .find(|product| product.name == record.product)
            .map_or(&[][..], |product| product.dimensions.as_slice());
        let mut values: Vec<_> = record.entities.0.iter().collect();
        values.sort_by_key(|(dimension, _)| {
            declared
                .iter()
                .position(|declared| declared == *dimension)
                .unwrap_or(usize::MAX)
        });
        let values: Vec<_> = values
            .into_iter()
            .map(|(dimension, value)| format!("{dimension}={value}"))
            .collect();
        text.push_str(&format!("    {}[{}]\n", record.product, values.join(",")));
    }
    text
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
    let syntax = "expected constraint: require product count=1 per [dimensions], count>=1, or dimension=value,...";
    let rest = line
        .strip_prefix("require ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let (subject, dimensions) = rest
        .split_once(" per ")
        .ok_or_else(|| ParseError::new(number, syntax))?;
    let mut parts = subject.split_whitespace();
    let product = qualified_identifier(parts.next().unwrap_or(""), number, "constraint product")?;
    let mut count = None;
    let mut values = BTreeMap::new();
    for token in parts {
        let parsed = if let Some(value) = token.strip_prefix("count=") {
            Some(CountRequirement::Exactly(parse_count(value, number)?))
        } else if let Some(value) = token.strip_prefix("count>=") {
            Some(CountRequirement::AtLeast(parse_count(value, number)?))
        } else {
            None
        };
        if let Some(parsed) = parsed {
            if count.replace(parsed).is_some() {
                return Err(ParseError::new(number, "a rule takes one count").at_token(token));
            }
        } else if let Some((dimension, listed)) = token.split_once('=') {
            let dimension = identifier(dimension, number, "required dimension")?;
            let listed: Vec<_> = listed.split(',').collect();
            if listed.iter().any(|value| value.is_empty()) {
                return Err(
                    ParseError::new(number, "required values must not be empty").at_token(token)
                );
            }
            if values.insert(dimension.to_owned(), listed).is_some() {
                return Err(ParseError::new(
                    number,
                    format!("duplicate required dimension `{dimension}`"),
                )
                .at_token(token));
            }
        } else {
            return Err(ParseError::new(number, syntax).at_token(token));
        }
    }
    if count.is_none() && values.is_empty() {
        return Err(ParseError::new(number, syntax));
    }
    let bracketed = dimensions.trim();
    let dimensions = bracketed.strip_prefix('[').ok_or_else(|| {
        ParseError::new(number, "expected `[` before constraint dimensions").at_token(bracketed)
    })?;
    let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
        ParseError::new(number, "expected closing `]` in constraint dimensions").at_token(bracketed)
    })?;
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
    let mut rule = CoverageRule::new(
        product,
        &dimensions,
        count.unwrap_or(CountRequirement::AtLeast(1)),
    );
    for (dimension, listed) in values {
        rule = rule.requiring(dimension, listed);
    }
    Ok(rule)
}

fn parse_count(value: &str, number: usize) -> Result<usize, ParseError> {
    value.parse().map_err(|_| {
        ParseError::new(number, "constraint count must be a nonnegative integer").at_token(value)
    })
}

/// Turn a [`TypeParseError`] into a [`ParseError`] pointing at the specific
/// token within `ty` that the type parser rejected, rather than all of `ty`.
fn type_error(number: usize, ty: &str, error: TypeParseError) -> ParseError {
    ParseError::new(number, error.message).at_token(&ty[error.span])
}

fn parse_product(line: &str, number: usize) -> Result<ProductDef, ParseError> {
    let (declaration, dimensions) = line
        .split_once('[')
        .ok_or_else(|| ParseError::new(number, "expected product name followed by [dimensions]"))?;
    let (name, artifact_type) = if let Some((name, ty)) = declaration.split_once(':') {
        let ty = ty.trim();
        let ty = parse_type_expr(ty, false).map_err(|error| type_error(number, ty, error))?;
        (name.trim(), ty)
    } else {
        (declaration.trim(), TypeExpr::Unknown)
    };
    let name = identifier(name, number, "product name")?;
    // From the `[` that is never closed to the end of the declaration.
    let bracketed = &line[declaration.len()..];
    let dimensions = dimensions.strip_suffix(']').ok_or_else(|| {
        ParseError::new(number, "expected closing `]` in product declaration").at_token(bracketed)
    })?;
    let dimensions = comma_items(dimensions, number)?;
    for dimension in &dimensions {
        identifier(dimension, number, "dimension")?;
    }
    Ok(ProductDef::new(name, artifact_type, &dimensions))
}

fn parse_operation(line: &str, number: usize) -> Result<OperationDef, ParseError> {
    let mut clauses = line.split('@');
    let line = clauses.next().unwrap_or_default().trim_end();
    let mut aggregated_dimension = None;
    let mut minimum = None;
    for clause in clauses {
        let clause = clause.trim();
        let (keyword, argument) = clause
            .split_once('(')
            .and_then(|(keyword, rest)| Some((keyword.trim(), rest.strip_suffix(')')?.trim())))
            .ok_or_else(|| {
                ParseError::new(
                    number,
                    "expected `@ drop(dimension)` or `@ min(count)` after operation signature",
                )
                .at_token(clause)
            })?;
        match keyword {
            "drop" if aggregated_dimension.is_none() => {
                aggregated_dimension = Some(identifier(argument, number, "aggregated dimension")?);
            }
            "min" if minimum.is_none() => {
                let count: usize = argument
                    .parse()
                    .ok()
                    .filter(|count| *count > 0)
                    .ok_or_else(|| {
                        ParseError::new(number, "`@ min(count)` needs a positive integer")
                            .at_token(argument)
                    })?;
                minimum = Some(count);
            }
            "drop" | "min" => {
                return Err(
                    ParseError::new(number, format!("duplicate `@ {keyword}(...)`"))
                        .at_token(keyword),
                )
            }
            _ => {
                return Err(ParseError::new(
                    number,
                    "expected `@ drop(dimension)` or `@ min(count)` after operation signature",
                )
                .at_token(keyword))
            }
        }
    }
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
    let mut ports = Vec::new();
    let count = inputs.len();
    for (index, input) in inputs.iter().enumerate() {
        let (declared_name, input) = if let Some((name, value)) = input.split_once(':') {
            let name = identifier(name.trim(), number, "input port")?;
            if name == DEFAULT_OUTPUT {
                return Err(ParseError::new(
                    number,
                    format!(
                        "input port name `{DEFAULT_OUTPUT}` is reserved for the operation output"
                    ),
                )
                .at_token(name));
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
        let port_name = declared_name
            .map(str::to_owned)
            .unwrap_or_else(|| DefaultPort::for_input(index, count).name());
        ports.push(match cardinality {
            Cardinality::One => InputPort::one(&port_name, artifact_type),
            Cardinality::Many => InputPort::many(&port_name, artifact_type),
        });
    }
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
    let shape_rule = if many == 1 {
        ShapeRule::Aggregate
    } else {
        ShapeRule::Preserve
    };
    if shape_rule != ShapeRule::Aggregate {
        if aggregated_dimension.is_some() {
            return Err(ParseError::new(
                number,
                "`@ drop(dimension)` requires a many input",
            ));
        }
        if minimum.is_some() {
            return Err(ParseError::new(
                number,
                "`@ min(count)` requires a many input",
            ));
        }
    }
    let mut operation = OperationDef::with_outputs(name, ports, outputs, shape_rule);
    if let Some(dimension) = aggregated_dimension {
        operation = operation.aggregating(dimension);
    }
    if let Some(minimum) = minimum {
        operation = operation.at_least(minimum);
    }
    Ok(operation)
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

fn parse_invocation(line: &str, number: usize) -> Result<Invocation, ParseError> {
    let (outputs, call) = line
        .split_once('=')
        .ok_or_else(|| ParseError::new(number, "expected `=` in pipeline invocation"))?;
    let outputs = comma_items(outputs, number)?
        .into_iter()
        .map(|output| identifier(output, number, "output product").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    if outputs.is_empty() {
        return Err(ParseError::new(
            number,
            "expected an output product before `=`",
        ));
    }
    parse_invocation_parts(outputs, call, number)
}

fn parse_invocation_parts(
    outputs: Vec<String>,
    call: &str,
    number: usize,
) -> Result<Invocation, ParseError> {
    let (operation, args) = call_parts(call.trim(), number)?;
    let bindings = comma_items(args, number)?
        .into_iter()
        .map(|arg| parse_binding(arg, number))
        .collect::<Result<_, _>>()?;
    Ok(Invocation::with_outputs(operation, bindings, outputs))
}

const SELECTORS: &str = "expected `@ vary(dimension)`, `@ where(dimension=value, ...)`, `@ same(dimension, ...)`, or `@ each(dimension, ...)`";

/// Parse `product [@ selector(...)]...`.
fn parse_binding(arg: &str, number: usize) -> Result<InputBinding, ParseError> {
    let mut parts = arg.split('@');
    let product = parts.next().unwrap_or_default().trim();
    let mut binding =
        InputBinding::product(qualified_identifier(product, number, "input product")?);
    for selector in parts {
        let selector = selector.trim();
        let (keyword, arguments) = call_parts(selector, number)
            .map_err(|_| ParseError::new(number, SELECTORS).at_token(selector))?;
        let items = comma_items(arguments, number)?;
        if items.is_empty() {
            return Err(
                ParseError::new(number, format!("`@ {keyword}()` needs a dimension"))
                    .at_token(selector),
            );
        }
        let duplicate =
            || ParseError::new(number, format!("duplicate `@ {keyword}(...)`")).at_token(keyword);
        match keyword {
            "vary" => {
                let [dimension] = items.as_slice() else {
                    return Err(ParseError::new(number, "`@ vary(...)` takes one dimension")
                        .at_token(selector));
                };
                let dimension = identifier(dimension, number, "vary dimension")?;
                if binding.vary.replace(dimension.to_owned()).is_some() {
                    return Err(duplicate());
                }
            }
            "where" => {
                if !binding.pinned.is_empty() {
                    return Err(duplicate());
                }
                for item in items {
                    let (dimension, value) = item.split_once('=').ok_or_else(|| {
                        ParseError::new(number, "expected `dimension=value` in `@ where(...)`")
                            .at_token(item)
                    })?;
                    let dimension = identifier(dimension.trim(), number, "where dimension")?;
                    let value = value.trim();
                    if value.is_empty() || value.chars().any(char::is_whitespace) {
                        return Err(ParseError::new(
                            number,
                            "a `@ where` value must be one nonempty token",
                        )
                        .at_token(item));
                    }
                    if binding
                        .pinned
                        .insert(dimension.to_owned(), value.to_owned())
                        .is_some()
                    {
                        return Err(ParseError::new(
                            number,
                            format!("`@ where(...)` pins `{dimension}` twice"),
                        )
                        .at_token(item));
                    }
                }
            }
            "same" => {
                let dimensions = items
                    .into_iter()
                    .map(|item| identifier(item, number, "same dimension"))
                    .collect::<Result<Vec<_>, _>>()?;
                if binding.same.replace(owned(&dimensions)).is_some() {
                    return Err(duplicate());
                }
            }
            "each" => {
                if !binding.each.is_empty() {
                    return Err(duplicate());
                }
                for item in items {
                    let dimension = identifier(item, number, "each dimension")?;
                    if binding.each.iter().any(|each| each == dimension) {
                        return Err(ParseError::new(
                            number,
                            format!("`@ each(...)` names `{dimension}` twice"),
                        )
                        .at_token(item));
                    }
                    binding.each.push(dimension.to_owned());
                }
            }
            _ => return Err(ParseError::new(number, SELECTORS).at_token(keyword)),
        }
    }
    Ok(binding)
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
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

/// Parse `dimension=value, ...]`, the text after a record's opening `[`.
fn parse_bindings(bindings: &str, number: usize) -> Result<EntityBinding, ParseError> {
    let open = bindings.trim_end();
    let bindings = open.strip_suffix(']').ok_or_else(|| {
        let error = ParseError::new(number, "expected closing `]` in source artifact");
        if open.is_empty() {
            error
        } else {
            error.at_token(open)
        }
    })?;
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
            .at_token(item));
        }
        if values
            .insert(dimension.to_owned(), value.to_owned())
            .is_some()
        {
            return Err(ParseError::new(
                number,
                format!("duplicate source dimension `{dimension}`"),
            )
            .at_token(item));
        }
    }
    Ok(EntityBinding(values))
}

fn call_parts(line: &str, number: usize) -> Result<(&str, &str), ParseError> {
    let (name, args) = line
        .split_once('(')
        .ok_or_else(|| ParseError::new(number, "expected `(` in operation call"))?;
    // From the `(` that is never closed to the end of the call.
    let opened = &line[name.len()..];
    let name = qualified_identifier(name.trim(), number, "operation name")?;
    let args = args
        .strip_suffix(')')
        .ok_or_else(|| ParseError::new(number, "expected closing `)`").at_token(opened))?;
    Ok((name, args))
}

fn comma_items(text: &str, number: usize) -> Result<Vec<&str>, ParseError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    // The byte index of each opener not yet matched by a closer of its kind,
    // so an unexpected or unclosed bracket can point at the exact character.
    let mut brackets: Vec<usize> = Vec::new();
    let mut parens: Vec<usize> = Vec::new();
    let mut angles: Vec<usize> = Vec::new();
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '[' => brackets.push(index),
            ']' => {
                if brackets.pop().is_none() {
                    return Err(
                        ParseError::new(number, "unexpected `]`").at_token(&text[index..=index])
                    );
                }
            }
            '(' => parens.push(index),
            ')' => {
                if parens.pop().is_none() {
                    return Err(
                        ParseError::new(number, "unexpected `)`").at_token(&text[index..=index])
                    );
                }
            }
            '<' => angles.push(index),
            '>' => {
                if angles.pop().is_none() {
                    return Err(
                        ParseError::new(number, "unexpected `>`").at_token(&text[index..=index])
                    );
                }
            }
            ',' if parens.is_empty() && angles.is_empty() && brackets.is_empty() => {
                items.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if let Some((opener, &index)) = [
        ('(', parens.first()),
        ('<', angles.first()),
        ('[', brackets.first()),
    ]
    .into_iter()
    .find_map(|(opener, index)| index.map(|index| (opener, index)))
    {
        return Err(
            ParseError::new(number, format!("unclosed `{opener}`")).at_token(&text[index..=index])
        );
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
        .at_token(value));
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
