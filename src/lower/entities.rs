//! Locate entity format definitions and validate labels after dimensions exist.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Pipeline;
use crate::parser::{EntitiesDeclaration, ParseError};
use crate::span::Place;

#[derive(Default)]
pub(super) struct EntitiesDeclarations {
    format: Option<Place>,
    labels: BTreeMap<String, Place>,
}

impl EntitiesDeclarations {
    /// A rejected declaration changes nothing, so recovery can continue.
    pub(super) fn add(
        &mut self,
        pipeline: &mut Pipeline,
        declaration: &EntitiesDeclaration,
        place: &Place,
    ) -> Result<(), ParseError> {
        match declaration {
            EntitiesDeclaration::Format(format) => {
                if self.format.is_some() {
                    return Err(ParseError::new(
                        place.line,
                        "a pipeline has one `entities:` format",
                    )
                    .within(place));
                }
                let labels = pipeline
                    .entities_format
                    .take()
                    .map(|format| format.labels)
                    .unwrap_or_default();
                let mut format = format.clone();
                format.labels = labels;
                pipeline.entities_format = Some(format);
                self.format = Some(place.clone());
            }
            EntitiesDeclaration::Label { dimension, label } => {
                if self.labels.contains_key(dimension) {
                    return Err(ParseError::new(
                        place.line,
                        format!("duplicate entity label for dimension `{dimension}`"),
                    )
                    .within(place));
                }
                pipeline
                    .entities_format
                    .get_or_insert_with(Default::default)
                    .labels
                    .insert(dimension.clone(), label.clone());
                self.labels.insert(dimension.clone(), place.clone());
            }
        }
        Ok(())
    }

    pub(super) fn validate(&self, pipeline: &Pipeline) -> Result<(), ParseError> {
        let Some(format) = &pipeline.entities_format else {
            return Ok(());
        };
        let dimensions = pipeline.dimension_order();
        for (dimension, place) in &self.labels {
            if !dimensions.contains(dimension) {
                return Err(ParseError::new(
                    place.line,
                    format!("entity label names unknown dimension `{dimension}`"),
                )
                .within(place));
            }
        }
        let mut labels = BTreeSet::new();
        for dimension in &dimensions {
            let label = format.labels.get(dimension).unwrap_or(dimension);
            if !labels.insert(label) {
                let place = self
                    .labels
                    .get(dimension)
                    .or_else(|| {
                        self.labels
                            .iter()
                            .find(|(name, _)| format.labels.get(*name) == Some(label))
                            .map(|(_, place)| place)
                    })
                    .expect("a repeated entity label has at least one alias declaration");
                return Err(ParseError::new(
                    place.line,
                    format!("entity label `{label}` is used for more than one dimension"),
                )
                .within(place));
            }
        }
        Ok(())
    }
}
