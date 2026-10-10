//! Lower `with` lines: the properties a backend reads from jobs.

use crate::model::Prop;
use crate::parser::{ParseError, WithTarget};
use crate::span::Place;

use super::PipelineBuilder;

impl PipelineBuilder {
    /// Add a `with` line's properties to what it names. A line that repeats
    /// one already given fails before it changes anything.
    pub(super) fn add_with(
        &mut self,
        target: &WithTarget,
        stage: Option<&str>,
        props: &[Prop],
        place: &Place,
    ) -> Result<(), ParseError> {
        let line = place.line;
        let taken = |what: String| ParseError::new(line, format!("duplicate {what}"));
        match target {
            WithTarget::Scope => {
                let (slot, whose) = match stage {
                    Some(stage) => (
                        &mut self
                            .pipeline
                            .stages
                            .iter_mut()
                            .find(|definition| definition.name == stage)
                            .expect("a stage is declared before its lines")
                            .with,
                        format!(" for stage `{stage}`"),
                    ),
                    None => (&mut self.pipeline.with, String::new()),
                };
                if !slot.is_empty() {
                    return Err(taken(format!("`with:`{whose}")));
                }
                *slot = props.to_vec();
            }
            WithTarget::Operation(name) => {
                let Some(&at) = self.operation_at.get(name) else {
                    return Err(ParseError::new(
                        line,
                        format!("`with operation` names `{name}`, which is not declared above this line; declare an operation before its `with` line"),
                    ));
                };
                let operation = &mut self.pipeline.operations[at];
                if !operation.steps.is_empty() {
                    return Err(ParseError::new(
                        line,
                        format!("operation `{name}` is carried out by the steps in its body; put `with` on the operations those steps call, or on the stage"),
                    ));
                }
                if !operation.with.is_empty() {
                    return Err(taken(format!("`with operation {name}`")));
                }
                operation.with = props.to_vec();
            }
            WithTarget::Product(name) => {
                if self.pipeline.product_with.contains_key(name) {
                    return Err(taken(format!("`with product {name}`")));
                }
                self.lines.with_products.insert(name.clone(), place.clone());
                self.pipeline
                    .product_with
                    .insert(name.clone(), props.to_vec());
            }
        }
        Ok(())
    }

    /// A `with product` line must name a product some step makes. The
    /// product may be declared on either side of the line.
    pub(super) fn check_with_products(&self) -> Result<(), ParseError> {
        for (name, place) in &self.lines.with_products {
            let made = self
                .pipeline
                .invocations
                .iter()
                .any(|invocation| invocation.outputs.contains(name));
            if !made {
                return Err(ParseError::new(
                    place.line,
                    format!("`with product` names `{name}`, which no step makes; a source has no job to give properties to"),
                )
                .within(place));
            }
        }
        Ok(())
    }
}
