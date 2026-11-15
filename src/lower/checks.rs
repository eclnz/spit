//! The checks `lower` makes of the whole document, after each statement is
//! lowered: what no single statement can see.

use crate::parser::error::LowerError;
use crate::parser::ParseError;

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
