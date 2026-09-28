//! The flow form: `source` and `operation` declarations, steps written
//! `outputs = operation(inputs)`, and stages whose lines are indented
//! beneath a `stage name:` header.

use crate::model::{CommandRole, Invocation};

use super::declarations::{
    parse_command, parse_coverage_rule, parse_invocation_parts, parse_path, parse_product,
};
use super::lexical::{comma_items, identifier, strip_comment};
use super::operation::parse_operation;
use super::source_map::{name_place, rule_place, step_place, tail_place};
use super::{FlowOutput, FlowStep, ParseError, StatementKind, Syntax, SHELL_SOURCE_REMOVED};

pub(super) fn parse_flow(text: &str) -> Result<Syntax, ParseError> {
    let mut syntax = Syntax::default();
    let mut stages = OpenStages::default();
    for (index, original) in text.lines().enumerate() {
        flow_line(&mut syntax, &mut stages, original, index + 1)
            .map_err(|error| error.locate(original))?;
    }
    Ok(syntax)
}

/// Whether a line opens a stage, as opposed to a step whose output product
/// happens to be called `stage`.
pub(super) fn is_stage_header(line: &str) -> bool {
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
