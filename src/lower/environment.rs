//! Environment declarations attach tool and path dependencies to operations.

use super::PipelineBuilder;
use crate::parser::{ParseError, Statement, StatementKind};

/// Apply a tool environment, dependency list or anchored operation mark.
pub(super) fn apply(
    builder: &mut PipelineBuilder,
    statement: &Statement,
) -> Result<(), ParseError> {
    match &statement.kind {
        StatementKind::Tools(file) => {
            if builder.pipeline.tools_file.is_some() {
                return Err(ParseError::new(
                    statement.place.line,
                    "give tools from once",
                ));
            }
            builder.pipeline.tools_file = Some((file.clone(), statement.place.line));
        }
        StatementKind::Tool(name) => {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                || builder.pipeline.tool_names.contains(name)
            {
                return Err(ParseError::new(
                    statement.place.line,
                    "invalid or duplicate tool name",
                ));
            }
            builder.pipeline.tool_names.push(name.clone());
        }
        StatementKind::Uses(name, tools) => {
            for tool in tools {
                if !builder
                    .pipeline
                    .tool_names
                    .iter()
                    .any(|known| known == tool.trim_start_matches('!'))
                {
                    return Err(ParseError::new(
                        statement.place.line,
                        format!("uses names undeclared tool `{tool}`; declare it with tool"),
                    ));
                }
            }
            let operation = builder
                .pipeline
                .operations
                .iter_mut()
                .find(|op| &op.name == name)
                .ok_or_else(|| {
                    ParseError::new(
                        statement.place.line,
                        format!("uses names unknown operation `{name}`"),
                    )
                })?;
            if operation.tools.is_some() || !operation.steps.is_empty() {
                return Err(ParseError::new(
                    statement.place.line,
                    "give uses once on an operation run by a command",
                ));
            }
            operation.tools = Some(tools.clone());
        }
        StatementKind::Anchored(names) => {
            for name in names {
                let operation = builder
                    .pipeline
                    .operations
                    .iter_mut()
                    .find(|op| &op.name == name)
                    .ok_or_else(|| {
                        ParseError::new(
                            statement.place.line,
                            format!("anchored names unknown operation `{name}`; declare it first"),
                        )
                    })?;
                if !operation.steps.is_empty() || operation.anchored {
                    return Err(ParseError::new(
                        statement.place.line,
                        format!(
                            "`{name}` needs one anchored mark on an operation run by a command"
                        ),
                    ));
                }
                operation.anchored = true;
            }
        }
        _ => unreachable!("environment declarations are dispatched by lower"),
    }
    Ok(())
}
