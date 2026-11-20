//! The checks `lower` makes of the whole document, after each statement is
//! lowered: what no single statement can see.

use crate::model::{DefaultChecks, ExtensionChecks};
use crate::parser::error::LowerError;
use crate::parser::ParseError;
use crate::span::Place;

use super::PipelineBuilder;

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
                    name: product.name.to_string(),
                    sibling: beside.sibling.to_string(),
                },
            )
            .within(place));
        }
    }
    Ok(())
}

impl PipelineBuilder {
    /// Add a `check:` list. One that repeats the list already given fails
    /// before it changes anything.
    pub(super) fn add_default_checks(
        &mut self,
        stage: Option<&str>,
        extension: Option<&str>,
        checks: &DefaultChecks,
        place: &Place,
    ) -> Result<(), ParseError> {
        let Self {
            pipeline, lines, ..
        } = self;
        let (slot, whose) = match stage {
            Some(stage) => (
                &mut pipeline
                    .stages
                    .iter_mut()
                    .find(|definition| definition.name == stage)
                    .expect("a stage is declared before its lines")
                    .checks,
                Some(stage.to_owned()),
            ),
            None => (&mut pipeline.default_checks, None),
        };
        let repeated = match extension {
            Some(extension) => slot
                .by_extension
                .iter()
                .any(|listed| listed.extension == extension),
            None => {
                let kind = checks
                    .checks
                    .iter()
                    .chain(&checks.exempt)
                    .next()
                    .map(|used| used.kind);
                slot.checks
                    .iter()
                    .chain(&slot.exempt)
                    .any(|used| Some(used.kind) == kind)
            }
        };
        if repeated {
            return Err(ParseError::of(
                place.line,
                LowerError::DuplicateDefaultChecks {
                    stage: whose,
                    extension: extension.map(str::to_owned),
                },
            )
            .within(place));
        }
        match extension {
            Some(extension) => slot.by_extension.push(ExtensionChecks {
                extension: extension.to_owned(),
                checks: checks.checks.clone(),
                exempt: checks.exempt.clone(),
            }),
            None => {
                slot.checks.extend_from_slice(&checks.checks);
                slot.exempt.extend_from_slice(&checks.exempt);
            }
        }
        let placed = match stage {
            Some(stage) => lines.stage_checks.entry(stage.to_owned()).or_default(),
            None => lines.default_checks.get_or_insert_with(Place::default),
        };
        *placed = place.clone();
        Ok(())
    }
}
