use crate::model::{stage_and_parents, Pipeline, ToolList, ToolSet};
use crate::parser::error::LowerError;
use crate::parser::{ParseError, SourceMap, StatementKind};
use crate::span::Place;

#[derive(Default)]
pub(super) struct ToolLines {
    declared: Vec<String>,
    uses: Vec<Place>,
    defaults: Vec<(Option<String>, Place)>,
    anchored: Vec<(String, Place)>,
}

impl ToolLines {
    pub(super) fn add(
        &mut self,
        pipeline: &mut Pipeline,
        kind: &StatementKind,
        line: usize,
    ) -> Result<(), ParseError> {
        match kind {
            StatementKind::Tool(names, place) => {
                if let Some(name) = names.iter().find(|name| self.declared.contains(name)) {
                    return Err(ParseError::of(
                        line,
                        LowerError::ToolDeclaredTwice {
                            name: name.to_string(),
                        },
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
                    return Err(ParseError::of(
                        line,
                        LowerError::DuplicateUses {
                            operation: (uses.operation).to_string(),
                        },
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
                        Some(stage.to_owned()),
                    ),
                    None => (&mut pipeline.default_uses, None),
                };
                if slot.is_some() {
                    return Err(ParseError::of(
                        line,
                        LowerError::DuplicateDefaultUses { stage: whose },
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
                    return Err(ParseError::of(
                        line,
                        LowerError::AnchoredRepeated {
                            name: name.to_string(),
                        },
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

    pub(super) fn apply(
        &self,
        pipeline: &mut Pipeline,
        lines: &SourceMap,
    ) -> Result<(), ParseError> {
        let undeclared = |list: &ToolList, place: &Place| -> Result<(), ParseError> {
            match list
                .add
                .iter()
                .chain(&list.remove)
                .find(|name| !pipeline.tools.contains(name))
            {
                Some(name) => Err(ParseError::of(
                    place.line,
                    LowerError::ToolNotDeclared {
                        name: name.to_string(),
                    },
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
                return Err(ParseError::of(
                    place.line,
                    LowerError::UsesOnImportedOperation {
                        name: name.to_string(),
                    },
                )
                .within(place));
            }
            if !pipeline
                .operations
                .iter()
                .any(|operation| &operation.name == name)
            {
                return Err(ParseError::of(
                    place.line,
                    LowerError::UsesUndeclaredOperation {
                        name: name.to_string(),
                    },
                )
                .within(place));
            }
            undeclared(&uses.tools, place)?;
        }
        for (name, place) in &self.anchored {
            if lines.imported.contains(name) {
                return Err(ParseError::of(
                    place.line,
                    LowerError::AnchoredOnImportedOperation {
                        name: name.to_string(),
                    },
                )
                .within(place));
            }
            let operation = pipeline
                .operations
                .iter_mut()
                .find(|operation| &operation.name == name)
                .ok_or_else(|| {
                    ParseError::of(
                        place.line,
                        LowerError::AnchoredUndeclaredOperation {
                            name: name.to_string(),
                        },
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
