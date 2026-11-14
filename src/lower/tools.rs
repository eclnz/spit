//! Lowering `tool`, `uses`, `default uses:` and `anchored` lines: each
//! operation the file declares gets the tools its jobs depend on, and
//! whether its tools store their own paths.

use crate::model::{stage_and_parents, Pipeline, ToolList, ToolSet};
use crate::parser::{ParseError, SourceMap, StatementKind};
use crate::span::Place;

/// Where the file's tool lines are, for errors found once every line is
/// read.
#[derive(Default)]
pub(super) struct ToolLines {
    /// The tools the file's own `tool` lines declare.
    declared: Vec<String>,
    /// Where each of `Pipeline::uses` names its operation.
    uses: Vec<Place>,
    /// Each `default uses:` list, by its stage, and where it is.
    defaults: Vec<(Option<String>, Place)>,
    /// Each operation an `anchored` line names, and where the line is.
    anchored: Vec<(String, Place)>,
}

impl ToolLines {
    /// Add a `tool`, `uses`, `default uses:` or `anchored` line to
    /// `pipeline`. Any other statement is left alone.
    pub(super) fn add(
        &mut self,
        pipeline: &mut Pipeline,
        kind: &StatementKind,
        line: usize,
    ) -> Result<(), ParseError> {
        match kind {
            StatementKind::Tool(names, place) => {
                if let Some(name) = names.iter().find(|name| self.declared.contains(name)) {
                    return Err(ParseError::new(
                        line,
                        format!("tool `{name}` is already declared"),
                    )
                    .within(place));
                }
                for name in names {
                    self.declared.push(name.clone());
                    if !pipeline.tools.contains(name) {
                        pipeline.tools.push(name.clone());
                    }
                }
            }
            StatementKind::Uses(uses, place) => {
                if pipeline
                    .uses
                    .iter()
                    .any(|earlier| earlier.operation == uses.operation)
                {
                    return Err(ParseError::new(
                        line,
                        format!(
                            "duplicate `uses` line for operation `{}`; write its tools in one line",
                            uses.operation
                        ),
                    )
                    .within(place));
                }
                pipeline.uses.push(uses.clone());
                self.uses.push(place.clone());
            }
            StatementKind::DefaultUses {
                stage,
                tools,
                place,
            } => {
                let (slot, whose) = match stage {
                    Some(stage) => (
                        &mut pipeline
                            .stages
                            .iter_mut()
                            .find(|definition| &definition.name == stage)
                            .expect("a stage is declared before its lines")
                            .uses,
                        format!(" for stage `{stage}`"),
                    ),
                    None => (&mut pipeline.default_uses, String::new()),
                };
                if slot.is_some() {
                    return Err(ParseError::new(
                        line,
                        format!(
                            "duplicate `default uses:` list{whose}; write the tools in one line"
                        ),
                    )
                    .within(place));
                }
                *slot = Some(tools.clone());
                self.defaults.push((stage.clone(), place.clone()));
            }
            StatementKind::Anchored(names, place) => {
                if let Some(name) = names
                    .iter()
                    .find(|name| self.anchored.iter().any(|(earlier, _)| earlier == *name))
                {
                    return Err(ParseError::new(
                        line,
                        format!("operation `{name}` is already marked `anchored`"),
                    )
                    .within(place));
                }
                self.anchored
                    .extend(names.iter().map(|name| (name.clone(), place.clone())));
            }
            _ => {}
        }
        Ok(())
    }

    /// Check every list against the declared tools and operations, mark each
    /// operation an `anchored` line names, then give each operation the file
    /// declares its [`ToolSet`]: the file's default, then each stage's from
    /// the outermost, then its own `uses` line. An imported operation keeps
    /// the set and the mark its own file gave it, and the tools it names join
    /// `pipeline.tools`.
    pub(super) fn apply(
        &self,
        pipeline: &mut Pipeline,
        lines: &SourceMap,
    ) -> Result<(), ParseError> {
        let undeclared = |list: &ToolList, place: &Place| -> Result<(), ParseError> {
            match list.add.iter().chain(&list.remove).find(|name| !pipeline.tools.contains(name)) {
                Some(name) => Err(ParseError::new(
                    place.line,
                    format!("tool `{name}` is not declared; add `tool {name}`, and say what it is in the recipe's `.spitenv`"),
                )
                .within(place)),
                None => Ok(()),
            }
        };
        for (stage, place) in &self.defaults {
            let list = match stage {
                Some(stage) => pipeline
                    .stages
                    .iter()
                    .find(|definition| &definition.name == stage)
                    .and_then(|definition| definition.uses.as_ref()),
                None => pipeline.default_uses.as_ref(),
            };
            if let Some(list) = list {
                undeclared(list, place)?;
            }
        }
        for (uses, place) in pipeline.uses.iter().zip(&self.uses) {
            let name = &uses.operation;
            if lines.imported.contains(name) {
                return Err(ParseError::new(
                    place.line,
                    format!("operation `{name}` is imported; its tools are set by `uses` in the file that declares it"),
                )
                .within(place));
            }
            if !pipeline
                .operations
                .iter()
                .any(|operation| &operation.name == name)
            {
                return Err(ParseError::new(
                    place.line,
                    format!("`uses` names operation `{name}`, which is not declared"),
                )
                .within(place));
            }
            undeclared(&uses.tools, place)?;
        }
        for (name, place) in &self.anchored {
            // The mark travels with the operation, so only its own file sets it.
            if lines.imported.contains(name) {
                return Err(ParseError::new(
                    place.line,
                    format!("operation `{name}` is imported; mark it `anchored` in the file that declares it"),
                )
                .within(place));
            }
            let operation = pipeline
                .operations
                .iter_mut()
                .find(|operation| &operation.name == name)
                .ok_or_else(|| {
                    ParseError::new(
                        place.line,
                        format!("`anchored` names operation `{name}`, which is not declared"),
                    )
                    .within(place)
                })?;
            operation.anchored = true;
        }
        let mut imported_tools = Vec::new();
        for operation in &mut pipeline.operations {
            if lines.imported.contains(&operation.name) {
                imported_tools.extend(operation.tools.names().iter().cloned());
                continue;
            }
            let mut set = ToolSet::default();
            if let Some(list) = &pipeline.default_uses {
                set = set.with(list);
            }
            if let Some(stage) = lines.operation_stages.get(&operation.name) {
                let mut around: Vec<&str> = stage_and_parents(stage).collect();
                around.reverse();
                for name in around {
                    let list = pipeline
                        .stages
                        .iter()
                        .find(|definition| definition.name == name)
                        .and_then(|definition| definition.uses.as_ref());
                    if let Some(list) = list {
                        set = set.with(list);
                    }
                }
            }
            if let Some(uses) = pipeline
                .uses
                .iter()
                .find(|uses| uses.operation == operation.name)
            {
                set = set.with(&uses.tools);
            }
            operation.tools = set;
        }
        for name in imported_tools {
            if !pipeline.tools.contains(&name) {
                pipeline.tools.push(name);
            }
        }
        Ok(())
    }
}
