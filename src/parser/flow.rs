//! The flow form: `source` and `operation` declarations, steps written
//! `outputs = operation(inputs)`, and stages whose lines are indented
//! beneath a `stage name:` header.

use crate::model::{CommandRole, Invocation};

use super::declarations::{
    parse_command, parse_coverage_rule, parse_discover, parse_invocation_parts, parse_path,
    parse_product,
};
use super::keyword::Keyword;
use super::lexical::{comma_items, identifier, strip_comment};
use super::operation::parse_operation;
use super::source_map::{name_place, rule_place, step_place, tail_place};
use super::{FlowOutput, FlowStep, ParseError, StatementKind, Syntax, SHELL_SOURCE_REMOVED};

pub(super) fn parse_flow(text: &str) -> Syntax {
    let mut syntax = Syntax::default();
    let mut stages = OpenStages::default();
    for (index, original) in text.lines().enumerate() {
        if let Err(error) = flow_line(&mut syntax, &mut stages, original, index + 1) {
            syntax.error = Some(error.locate(original));
            break;
        }
    }
    syntax
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
    let kind = match Keyword::split(line) {
        Some((Keyword::Stage, declaration)) => {
            return open_stage(syntax, stages, original, declaration, indent, number);
        }
        Some((Keyword::Use, _)) => {
            top_level_only("`use`")?;
            StatementKind::Import
        }
        Some((Keyword::Source, declaration)) => {
            top_level_only("`source`, which declares an input,")?;
            let declaration = declaration.trim();
            let product = parse_product(declaration, number)?;
            let place = name_place(original, number, declaration, &product.name);
            StatementKind::Product(product, place)
        }
        Some((Keyword::Discover, declaration)) => {
            top_level_only("`discover`")?;
            StatementKind::Discover(parse_discover(declaration.trim(), number)?)
        }
        Some((Keyword::Operation, declaration)) => {
            let declaration = declaration.trim();
            let operation = parse_operation(declaration, number)?;
            let place = name_place(original, number, declaration, &operation.name);
            StatementKind::Operation(operation, place)
        }
        Some((keyword @ (Keyword::Require | Keyword::Skip), _)) => {
            top_level_only(if keyword == Keyword::Require {
                "`require`, which checks sources,"
            } else {
                "`skip`, which filters sources,"
            })?;
            let rule = parse_coverage_rule(line, number)?;
            let place = rule_place(original, number, &rule);
            StatementKind::Constraint(rule, place)
        }
        Some((keyword @ (Keyword::Command | Keyword::Verify), declaration)) => {
            let role = if keyword == Keyword::Command {
                CommandRole::Run
            } else {
                CommandRole::Verify
            };
            let command = parse_command(declaration.trim(), number, role)?;
            let place = tail_place(original, number, command.template.as_str());
            StatementKind::Command(command, place)
        }
        Some((Keyword::ShellSource, _)) => {
            return Err(ParseError::new(number, SHELL_SOURCE_REMOVED));
        }
        Some((Keyword::Path, _)) => StatementKind::Path(parse_path(stage, original, line, number)?),
        None => flow_statement(original, line, number, stage)?,
    };
    syntax.push(original, number, kind);
    Ok(())
}

/// A line that starts with no keyword: a step, or a mistake.
fn flow_statement(
    original: &str,
    line: &str,
    number: usize,
    stage: Option<String>,
) -> Result<StatementKind, ParseError> {
    if line.contains('=') {
        let (mut invocation, outputs) = parse_flow_step(line, number)?;
        invocation.stage.clone_from(&stage);
        let step = step_place(original, number, &invocation);
        Ok(StatementKind::FlowStep(FlowStep {
            invocation,
            outputs,
            step,
        }))
    } else if line.contains('(') && line.ends_with(')') {
        Err(ParseError::new(
            number,
            "expected `=` before operation call",
        ))
    } else {
        Err(ParseError::new(
            number,
            "expected source, discover, operation, command, verify, require, path, stage, or output = operation(inputs)",
        ))
    }
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
