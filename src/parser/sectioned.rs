//! The sectioned form: declarations grouped under `products:`,
//! `operations:`, `pipeline:`, `constraints:` and `commands:` headers.

use crate::model::{CommandRole, Invocation};

use super::declarations::{parse_discover, parse_invocation_parts, parse_path};
use super::keyword::{Header, Keyword};
use super::lexical::{comma_items, identifier, strip_comment};
use super::source_map::step_place;
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
    text.lines()
        .map(strip_comment)
        .map(str::trim)
        .any(|line| Header::of(line).is_some_and(|header| !header.is_records()))
}

pub(super) fn parse_sectioned(text: &str) -> Syntax {
    let mut syntax = Syntax::default();
    let mut section = None;
    for (index, original) in text.lines().enumerate() {
        if let Err(error) = sectioned_line(&mut syntax, &mut section, original, index + 1) {
            syntax.error = Some(error.locate(original));
            break;
        }
    }
    syntax
}

fn sectioned_line(
    syntax: &mut Syntax,
    section: &mut Option<Section>,
    original: &str,
    number: usize,
) -> Result<(), ParseError> {
    let line = strip_comment(original).trim();
    let mut push = |kind| syntax.push(original, number, kind);
    if line.is_empty() {
        return Ok(());
    }
    if let Some(header) = Header::of(line) {
        *section = Some(match header {
            Header::Products => Section::Products,
            Header::Operations => Section::Operations,
            Header::Pipeline => Section::Pipeline,
            Header::Constraints => Section::Constraints,
            Header::Commands => Section::Commands,
            Header::Sources | Header::SourcePaths | Header::Contexts(_) => {
                return Err(ParseError::new(
                    number,
                    "`sources:`, `source_paths:`, and `contexts:` records belong in a .spitout, not a pipeline",
                ))
            }
        });
        return Ok(());
    }
    match Keyword::split(line) {
        Some((Keyword::Use, _)) => {
            push(StatementKind::Import);
            *section = None;
        }
        Some((Keyword::Discover, declaration)) => {
            let discovery = parse_discover(declaration.trim(), number)?;
            push(StatementKind::Discover(discovery));
            *section = None;
        }
        Some((Keyword::ShellSource, _)) => {
            return Err(ParseError::new(number, SHELL_SOURCE_REMOVED));
        }
        Some((Keyword::Stage, _)) => {
            return Err(ParseError::new(
                number,
                "stages are written in the flow form, not in a sectioned document",
            ));
        }
        Some((Keyword::Path, _)) => {
            push(StatementKind::Path(parse_path(
                None, original, line, number,
            )?));
            *section = None;
        }
        _ => {
            let Some(section) = *section else {
                return Err(ParseError::new(
                    number,
                    "expected a section header: products:, operations:, pipeline:, constraints:, or commands:",
                ));
            };
            push(section_statement(section, original, line, number)?);
        }
    }
    Ok(())
}

/// A line inside `section`, as the statement that section holds.
fn section_statement(
    section: Section,
    original: &str,
    line: &str,
    number: usize,
) -> Result<StatementKind, ParseError> {
    match section {
        Section::Products => StatementKind::product(original, line, number),
        Section::Operations => StatementKind::operation(original, line, number),
        Section::Pipeline => {
            let invocation = parse_invocation(line, number)?;
            let step = step_place(original, number, &invocation);
            Ok(StatementKind::Step(invocation, step))
        }
        Section::Constraints => StatementKind::constraint(original, line, number),
        Section::Commands => match line.strip_prefix("verify ") {
            Some(declaration) => {
                StatementKind::command(original, declaration.trim(), number, CommandRole::Verify)
            }
            None => StatementKind::command(original, line, number, CommandRole::Run),
        },
    }
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
