use std::collections::{BTreeMap, BTreeSet};

use crate::model::Pipeline;
use crate::parser::error::LowerError;
use crate::parser::{EntitiesDeclaration, ParseError};
use crate::span::Place;
use crate::template::EntitiesFormat;

type Scope = Option<String>;

#[derive(Default)]
pub(super) struct EntitiesDeclarations {
    formats: BTreeMap<Scope, Place>,
    labels: BTreeMap<Scope, BTreeMap<String, (String, Place)>>,
}

fn slot<'p>(pipeline: &'p mut Pipeline, scope: &Scope) -> &'p mut Option<EntitiesFormat> {
    match scope {
        Some(stage) => {
            &mut pipeline
                .stages
                .iter_mut()
                .find(|definition| definition.name == *stage)
                .expect("a stage is declared before its lines")
                .entities_format
        }
        None => &mut pipeline.entities_format,
    }
}

impl EntitiesDeclarations {
    pub(super) fn add(
        &mut self,
        pipeline: &mut Pipeline,
        stage: Option<&str>,
        declaration: &EntitiesDeclaration,
        place: &Place,
    ) -> Result<(), ParseError> {
        let scope = stage.map(str::to_owned);
        match declaration {
            EntitiesDeclaration::Format(format) => {
                if self.formats.contains_key(&scope) {
                    return Err(ParseError::of(
                        place.line,
                        LowerError::DuplicateDefaultEntities {
                            stage: scope.clone(),
                        },
                    )
                    .within(place));
                }
                *slot(pipeline, &scope) = Some(format.clone());
                self.formats.insert(scope, place.clone());
            }
            EntitiesDeclaration::Label { dimension, label } => {
                let labels = self.labels.entry(scope).or_default();
                if labels.contains_key(dimension) {
                    return Err(ParseError::of(
                        place.line,
                        LowerError::DuplicateEntityLabelDimension {
                            dimension: dimension.to_string(),
                        },
                    )
                    .within(place));
                }
                labels.insert(dimension.clone(), (label.clone(), place.clone()));
            }
        }
        Ok(())
    }

    pub(super) fn validate(&self, pipeline: &mut Pipeline) -> Result<(), ParseError> {
        let dimensions = pipeline.dimension_order();
        for (scope, labels) in &self.labels {
            let Some(format) = slot(pipeline, scope) else {
                let (_, place) = labels.values().next().expect("a scope with labels has one");
                return Err(ParseError::of(
                    place.line,
                    LowerError::EntityLabelWithoutSpelling {
                        stage: scope.clone(),
                    },
                )
                .within(place));
            };
            format.labels = labels
                .iter()
                .map(|(dimension, (label, _))| (dimension.clone(), label.clone()))
                .collect();
            if pipeline.products.is_empty() {
                continue;
            }
            for (dimension, (_, place)) in labels {
                if !dimensions.contains(dimension) {
                    return Err(ParseError::of(
                        place.line,
                        LowerError::EntityLabelNamesUnknownDimension {
                            dimension: dimension.to_string(),
                        },
                    )
                    .within(place));
                }
            }
            let mut seen = BTreeSet::new();
            for dimension in &dimensions {
                let label = labels.get(dimension).map_or(dimension, |(label, _)| label);
                if !seen.insert(label) {
                    let place = labels
                        .get(dimension)
                        .or_else(|| labels.values().find(|(other, _)| other == label))
                        .map(|(_, place)| place)
                        .expect("a repeated entity label has at least one alias declaration");
                    return Err(ParseError::of(
                        place.line,
                        LowerError::EntityLabelRepeated {
                            label: label.to_string(),
                        },
                    )
                    .within(place));
                }
            }
        }
        Ok(())
    }
}
