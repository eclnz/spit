//! Locate `default entities` spellings and labels, by the stage they are
//! written in, and check the labels once dimensions exist.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Pipeline;
use crate::parser::{EntitiesDeclaration, ParseError};
use crate::paths::EntitiesFormat;
use crate::span::Place;

/// A stage's name, or `None` for the file outside every stage.
type Scope = Option<String>;

#[derive(Default)]
pub(super) struct EntitiesDeclarations {
    /// Where each scope's spelling is declared.
    formats: BTreeMap<Scope, Place>,
    /// Each scope's relabelled dimensions, with each label and where it is
    /// declared. A label may come before the spelling it belongs to, so
    /// `validate` gives the labels to their spelling once every statement
    /// is lowered.
    labels: BTreeMap<Scope, BTreeMap<String, (String, Place)>>,
}

/// What a scope is called in a message.
fn whose(scope: &Scope) -> String {
    scope
        .as_ref()
        .map_or_else(String::new, |stage| format!(" for stage `{stage}`"))
}

/// The spelling of `scope` in `pipeline`, where it is kept.
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
    /// A rejected declaration changes nothing, so recovery can continue.
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
                    return Err(ParseError::new(
                        place.line,
                        format!("duplicate `default entities:`{}", whose(&scope)),
                    )
                    .within(place));
                }
                *slot(pipeline, &scope) = Some(format.clone());
                self.formats.insert(scope, place.clone());
            }
            EntitiesDeclaration::Label { dimension, label } => {
                let labels = self.labels.entry(scope).or_default();
                if labels.contains_key(dimension) {
                    return Err(ParseError::new(
                        place.line,
                        format!("duplicate entity label for dimension `{dimension}`"),
                    )
                    .within(place));
                }
                labels.insert(dimension.clone(), (label.clone(), place.clone()));
            }
        }
        Ok(())
    }

    /// Give each scope's labels to its spelling, and check them against the
    /// pipeline's dimensions. A recipe declares no products, so its labels
    /// are checked against its pipeline's when the two are read together.
    pub(super) fn validate(&self, pipeline: &mut Pipeline) -> Result<(), ParseError> {
        let dimensions = pipeline.dimension_order();
        for (scope, labels) in &self.labels {
            let Some(format) = slot(pipeline, scope) else {
                let (_, place) = labels.values().next().expect("a scope with labels has one");
                return Err(ParseError::new(
                    place.line,
                    format!(
                        "an entity label renames a dimension in a `default entities:` spelling, and there is none{}; add one, as `default entities: {{@key}}={{@value}} separated \"__\"`",
                        whose(scope)
                    ),
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
                    return Err(ParseError::new(
                        place.line,
                        format!("entity label names unknown dimension `{dimension}`"),
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
                    return Err(ParseError::new(
                        place.line,
                        format!("entity label `{label}` is used for more than one dimension"),
                    )
                    .within(place));
                }
            }
        }
        Ok(())
    }
}
