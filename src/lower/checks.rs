//! The checks `lower` makes of the whole document, after each statement is
//! lowered: what no single statement can see.

use crate::model::{CommandId, CommandRole};
use crate::parser::error::LowerError;
use crate::parser::ParseError;

use super::PipelineBuilder;

/// An operation with a body is carried out by its steps, so it takes no
/// `command` or `verify` line of its own.
pub(super) fn check_bodies_have_no_commands(builder: &PipelineBuilder) -> Result<(), ParseError> {
    let PipelineBuilder {
        pipeline,
        lines,
        operation_at,
        ..
    } = builder;
    for (index, command) in pipeline.commands.iter().enumerate() {
        let Some(&operation) = operation_at.get(&command.operation) else {
            continue;
        };
        let Some(place) = lines.command(CommandId::at(index)) else {
            continue;
        };
        if !pipeline.operations[operation].steps.is_empty() {
            return Err(ParseError::of(
                place.line,
                LowerError::CommandOnOperationWithSteps {
                    operation: (command.operation).to_string(),
                    keyword: (match command.role {
                        CommandRole::Run => "command",
                        CommandRole::Verify => "verify",
                    })
                    .to_string(),
                },
            )
            .within(&place));
        }
    }
    Ok(())
}

/// A source written beside another takes the other's path, so it has no
/// independent path rule in the pipeline.
pub(super) fn check_source_beside_paths(builder: &PipelineBuilder) -> Result<(), ParseError> {
    for product in &builder.pipeline.products {
        let Some(beside) = &product.beside else {
            continue;
        };
        if let Some(place) = builder.lines.paths.get(&product.name) {
            return Err(ParseError::of(
                place.line,
                LowerError::PathRuleOnBesideSource {
                    name: (product.name).to_string(),
                    sibling: (beside.sibling).to_string(),
                },
            )
            .within(place));
        }
    }
    Ok(())
}
