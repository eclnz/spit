//! The sectioned form: declarations grouped under `products:`,
//! `operations:`, `pipeline:`, `constraints:` and `commands:` headers.

use crate::model::{CommandRole, Invocation};

use super::declarations::{
    parse_command, parse_coverage_rule, parse_invocation_parts, parse_path, parse_product,
};
use super::flow::is_stage_header;
use super::lexical::{comma_items, identifier, strip_comment};
use super::operation::parse_operation;
use super::source_map::{name_place, rule_place, step_place, tail_place};
use super::{ParseError, StatementKind, Syntax, SHELL_SOURCE_REMOVED};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Section {
    Products,
    Operations,
    Pipeline,
    Constraints,
    Commands,
}

pub(super) fn is_sectioned_document(text: &str) -> bool {
    text.lines().map(strip_comment).map(str::trim).any(|line| {
        matches!(
            line,
            "products:" | "operations:" | "pipeline:" | "constraints:" | "commands:"
        )
    })
}

pub(super) fn parse_sectioned(text: &str) -> Result<Syntax, ParseError> {
    let mut syntax = Syntax::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        sectioned_line(&mut syntax, &mut section, original, index + 1)
            .map_err(|error| error.locate(original))?;
    }
    Ok(syntax)
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
