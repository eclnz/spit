//! Locate entity format definitions and validate labels after dimensions exist.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Pipeline;
use crate::parser::{EntitiesDeclaration, ParseError};
use crate::span::Place;

#[derive(Default)]
pub(super) struct EntitiesDeclarations {
    format: Option<Place>,
    /// Each relabelled dimension, its label and where it is declared. A
    /// label may come before the format it belongs to, so `validate` gives
    /// the labels to the format once every statement is lowered.
    labels: BTreeMap<String, (String, Place)>,
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
                pipeline.entities_format = Some(format.clone());
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
                self.labels
                    .insert(dimension.clone(), (label.clone(), place.clone()));
            }
        }
        Ok(())
    }

    /// Give the labels to the format, and check them against the
    /// pipeline's dimensions.
    pub(super) fn validate(&self, pipeline: &mut Pipeline) -> Result<(), ParseError> {
        let dimensions = pipeline.dimension_order();
        let Some(format) = &mut pipeline.entities_format else {
            return match self.labels.values().next() {
                Some((_, place)) => Err(ParseError::new(
                    place.line,
                    "an entity label renames a dimension in the `entities:` format, and this pipeline declares none; add one, as `entities: {key}={value} separated \"__\" empty \"global\"`",
                )
                .within(place)),
                None => Ok(()),
            };
        };
        format.labels = self
            .labels
            .iter()
            .map(|(dimension, (label, _))| (dimension.clone(), label.clone()))
            .collect();
        let format = &*format;
        for (dimension, (_, place)) in &self.labels {
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
                    .or_else(|| self.labels.values().find(|(other, _)| other == label))
                    .map(|(_, place)| place)
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
