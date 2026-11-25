use rustc_hash::FxHashMap;

use crate::model::{Call, CallId, Invocation, OperationDef, DEFAULT_OUTPUT};
use crate::parser::{source_record_lines, Rule, SourceMap, Step};
use crate::render::render_call;
use crate::span::{content_columns, Place};
use crate::{DefinitionSubject, EntityBinding, Pipeline, ResolveError};

use super::{DiagnosticSource, Related};

pub(super) struct Places<'a> {
    pipeline: &'a Pipeline,
    lines: &'a SourceMap,
    makers: FxHashMap<&'a str, &'a Invocation>,
    operations: FxHashMap<&'a str, &'a OperationDef>,
    readers: FxHashMap<&'a str, Vec<&'a Invocation>>,
    callers: FxHashMap<&'a str, Vec<&'a Invocation>>,
}

impl<'a> Places<'a> {
    pub(super) fn new(pipeline: &'a Pipeline, lines: &'a SourceMap) -> Self {
        let mut makers = FxHashMap::default();
        let mut readers: FxHashMap<_, Vec<_>> = FxHashMap::default();
        let mut callers: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for invocation in &pipeline.invocations {
            for output in &invocation.outputs {
                makers.entry(output.as_str()).or_insert(invocation);
            }
            for input in &invocation.inputs {
                readers
                    .entry(input.product_name())
                    .or_default()
                    .push(invocation);
            }
            callers
                .entry(invocation.operation.as_str())
                .or_default()
                .push(invocation);
        }
        let mut operations = FxHashMap::default();
        for operation in &pipeline.operations {
            operations
                .entry(operation.name.as_str())
                .or_insert(operation);
        }
        Self {
            pipeline,
            lines,
            makers,
            operations,
            readers,
            callers,
        }
    }

    fn readers(&self, product: &str) -> &[&'a Invocation] {
        self.readers.get(product).map_or(&[], Vec::as_slice)
    }

    fn operation(&self, name: &str) -> Option<&'a OperationDef> {
        self.operations.get(name).copied()
    }

    pub(super) fn subject_place(
        &self,
        subject: &DefinitionSubject,
        error: &ResolveError,
    ) -> Option<Place> {
        let lines = self.lines;
        match subject {
            DefinitionSubject::Product(name) => lines.products.get(name).cloned(),
            DefinitionSubject::Operation(name) => lines.operations.get(name).cloned(),
            DefinitionSubject::Invocation(output) => {
                let step = lines.invocations.get(output)?;
                Some(self.step_part(step, output, error))
            }
            DefinitionSubject::Constraint(index) => lines.rules.get(*index).map(Rule::product),
            DefinitionSubject::ConstraintGroup(index) => {
                lines.rules.get(*index).map(Rule::dimensions)
            }
            DefinitionSubject::Exclusion(index) => lines.exclusions.get(*index).cloned(),
            DefinitionSubject::ReviewFile(index) => lines.review_files.get(*index).cloned(),
            DefinitionSubject::Stage(name) => lines.stages.get(name).cloned(),
            DefinitionSubject::Call(_) => None,
            DefinitionSubject::Source(_) | DefinitionSubject::None => None,
        }
    }

    fn step_part(&self, step: &Step, output: &str, error: &ResolveError) -> Place {
        let invocation = self.makers.get(output).copied();
        let input_named = |product: &str| {
            let index = invocation?
                .inputs
                .iter()
                .position(|binding| binding.product_name() == product)?;
            step.input(index)
        };
        let port = |port: &str| {
            let operation = invocation.and_then(|invocation| self.operation(&invocation.operation));
            let Some(operation) = operation else {
                return (port == DEFAULT_OUTPUT).then(|| step.output());
            };
            if let Some(index) = operation
                .outputs
                .iter()
                .position(|output| output.name == port)
            {
                return Some(step.output_at(index));
            }
            let index = operation
                .inputs
                .iter()
                .position(|input| input.name == port)?;
            step.input(index)
        };
        let part = match error {
            ResolveError::TypeMismatch { site, .. }
            | ResolveError::TypeVariableConflict { site, .. }
            | ResolveError::MissingInput { site, .. }
            | ResolveError::AmbiguousInput { site, .. }
            | ResolveError::CollectionTooSmall { site, .. } => port(&site.port),
            ResolveError::UnknownProduct { name } if name == output => Some(step.output()),
            ResolveError::UnknownProduct { name } => input_named(name),
            ResolveError::UnknownOperation { .. } => Some(step.operation()),
            ResolveError::InvalidAggregationDimension { product, .. } => input_named(product),
            ResolveError::UnsupportedShapeRelationship { .. } => Some(step.call()),
            ResolveError::DuplicateSourceArtifact { .. }
            | ResolveError::DuplicateOutputArtifact { .. }
            | ResolveError::Cycle { .. }
            | ResolveError::CoverageViolation { .. }
            | ResolveError::MissingRequiredValue { .. }
            | ResolveError::NoGroupsToCheck { .. }
            | ResolveError::InvalidDefinition { .. } => Some(
                invocation
                    .and_then(|invocation| {
                        invocation.outputs.iter().position(|name| name == output)
                    })
                    .map_or_else(|| step.output(), |index| step.output_at(index)),
            ),
        };
        part.unwrap_or_else(|| step.call())
    }

    pub(super) fn error_location(
        &self,
        error: &ResolveError,
        inventory_text: &str,
        external_inventory: bool,
    ) -> (DiagnosticSource, Option<Place>) {
        if let Some(place) = self.pipeline_place(error) {
            return (DiagnosticSource::Pipeline, Some(place));
        }
        let inventory_place = inventory_place(error, inventory_text);
        match inventory_place {
            Some(_) if external_inventory => (DiagnosticSource::Inventory, inventory_place),
            _ => (DiagnosticSource::Pipeline, inventory_place),
        }
    }

    fn pipeline_place(&self, error: &ResolveError) -> Option<Place> {
        let lines = self.lines;
        let step = |output: &str| {
            let step = lines.invocations.get(output)?;
            Some(self.step_part(step, output, error))
        };
        let unique_step = |operation: &str| {
            let callers = self.callers.get(operation).map_or(&[][..], Vec::as_slice);
            let mut matching = callers
                .iter()
                .filter_map(|invocation| step(invocation.output_product()));
            let first = matching.next()?;
            matching.next().is_none().then_some(first)
        };
        match error {
            ResolveError::TypeMismatch { site, .. }
            | ResolveError::TypeVariableConflict { site, .. }
            | ResolveError::MissingInput { site, .. }
            | ResolveError::AmbiguousInput { site, .. }
            | ResolveError::CollectionTooSmall { site, .. } => step(&site.output_product),
            ResolveError::UnknownOperation { name } => unique_step(name),
            ResolveError::UnknownProduct { name } => self
                .readers(name)
                .first()
                .and_then(|invocation| step(invocation.output_product()))
                .or_else(|| lines.constraints.get(name).map(Rule::product)),
            ResolveError::InvalidAggregationDimension { product, dimension } => self
                .readers(product)
                .iter()
                .find(|invocation| {
                    invocation.inputs.iter().any(|binding| {
                        &binding.product == product && binding.vary.contains(dimension)
                    })
                })
                .and_then(|invocation| step(invocation.output_product())),
            ResolveError::Cycle { products } => products.first().and_then(|name| step(name)),
            ResolveError::UnsupportedShapeRelationship { operation, .. } => unique_step(operation),
            ResolveError::CoverageViolation { rule_index, .. }
            | ResolveError::MissingRequiredValue { rule_index, .. }
            | ResolveError::NoGroupsToCheck { rule_index, .. } => {
                lines.rules.get(*rule_index).map(Rule::whole)
            }
            ResolveError::DuplicateOutputArtifact { artifact } => step(&artifact.product),
            ResolveError::InvalidDefinition { subject, .. } => self.subject_place(subject, error),
            ResolveError::DuplicateSourceArtifact { .. } => None,
        }
    }
}

fn inventory_place(error: &ResolveError, inventory_text: &str) -> Option<Place> {
    let inventory_line = match error {
        ResolveError::UnknownProduct { name } => inventory_record_lines(inventory_text, name, None)
            .into_iter()
            .next(),
        ResolveError::DuplicateSourceArtifact { artifact } => {
            inventory_record_lines(inventory_text, &artifact.product, Some(&artifact.entities))
                .into_iter()
                .nth(1)
        }
        ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Source(record),
            ..
        } => inventory_record_lines(inventory_text, &record.product, Some(&record.entities))
            .into_iter()
            .next(),
        ResolveError::InvalidDefinition { .. }
        | ResolveError::UnknownOperation { .. }
        | ResolveError::TypeMismatch { .. }
        | ResolveError::TypeVariableConflict { .. }
        | ResolveError::MissingInput { .. }
        | ResolveError::AmbiguousInput { .. }
        | ResolveError::CollectionTooSmall { .. }
        | ResolveError::InvalidAggregationDimension { .. }
        | ResolveError::DuplicateOutputArtifact { .. }
        | ResolveError::Cycle { .. }
        | ResolveError::UnsupportedShapeRelationship { .. }
        | ResolveError::CoverageViolation { .. }
        | ResolveError::MissingRequiredValue { .. }
        | ResolveError::NoGroupsToCheck { .. } => None,
    };
    inventory_line.and_then(|line| {
        let text = inventory_text.lines().nth(line.checked_sub(1)?)?;
        Some(Place::new(line, content_columns(text)))
    })
}

fn inventory_record_lines(
    text: &str,
    product: &str,
    entities: Option<&EntityBinding>,
) -> Vec<usize> {
    source_record_lines(text)
        .into_iter()
        .filter_map(|(line, record)| {
            (record.product == product && entities.is_none_or(|value| value == &record.entities))
                .then_some(line)
        })
        .collect()
}

pub(super) struct InCall {
    pub(super) place: Place,
    pub(super) prefix: String,
    pub(super) related: Vec<Related>,
}

pub(super) fn error_step(error: &ResolveError) -> Option<(&str, Option<&str>)> {
    match error {
        ResolveError::TypeMismatch { site, .. }
        | ResolveError::TypeVariableConflict { site, .. }
        | ResolveError::MissingInput { site, .. }
        | ResolveError::AmbiguousInput { site, .. }
        | ResolveError::CollectionTooSmall { site, .. } => {
            Some((&site.output_product, Some(&site.port)))
        }
        ResolveError::UnknownProduct { .. }
        | ResolveError::UnknownOperation { .. }
        | ResolveError::InvalidAggregationDimension { .. }
        | ResolveError::DuplicateSourceArtifact { .. }
        | ResolveError::DuplicateOutputArtifact { .. }
        | ResolveError::Cycle { .. }
        | ResolveError::UnsupportedShapeRelationship { .. }
        | ResolveError::CoverageViolation { .. }
        | ResolveError::MissingRequiredValue { .. }
        | ResolveError::NoGroupsToCheck { .. }
        | ResolveError::InvalidDefinition { .. } => None,
    }
}

impl Places<'_> {
    pub(super) fn in_call(&self, output: &str, port: Option<&str>) -> Option<InCall> {
        let invocation = self.makers.get(output)?;
        let origin = invocation.origin.as_ref()?;
        let given = port.and_then(|port| {
            let called = self.operation(&invocation.operation)?;
            if let Some(index) = called.inputs.iter().position(|input| input.name == port) {
                return Some(Given::Input(&invocation.inputs.get(index)?.product));
            }
            let index = called
                .outputs
                .iter()
                .position(|output| output.name == port)?;
            Some(Given::Output(invocation.outputs.get(index)?))
        });
        self.reported(origin.call, given, Some(&origin.step))
    }

    pub(super) fn at_call(&self, call: CallId, port: Option<&str>) -> Option<InCall> {
        let written = self.pipeline.calls.get(call.index())?;
        let given = port.and_then(|port| {
            let called = self.operation(&written.operation)?;
            if let Some(index) = called.inputs.iter().position(|input| input.name == port) {
                return Some(Given::Input(written.inputs.get(index)?));
            }
            let index = called
                .outputs
                .iter()
                .position(|output| output.name == port)?;
            Some(Given::Output(written.outputs.get(index)?))
        });
        self.reported(call, given, None)
    }
}

enum Given<'a> {
    Input(&'a str),
    Output(&'a str),
}

impl Places<'_> {
    fn reported(&self, call: CallId, given: Option<Given>, step: Option<&Place>) -> Option<InCall> {
        let (pipeline, lines) = (self.pipeline, self.lines);
        let chain: Vec<&Call> = std::iter::successors(pipeline.calls.get(call.index()), |call| {
            call.parent.map(|parent| &pipeline.calls[parent.index()])
        })
        .collect();
        let root = chain.last()?;
        let written = lines.invocations.get(root.outputs.first()?)?;
        let part = given.and_then(|given| match given {
            Given::Input(product) => {
                written.input(root.inputs.iter().position(|input| input == product)?)
            }
            Given::Output(product) => {
                let at = root.outputs.iter().position(|output| output == product)?;
                Some(written.output_at(at))
            }
        });
        let related = |file: Option<String>, place: &Place, message: String| {
            let line_text = file.as_ref().map(|file| {
                lines
                    .file_texts
                    .get(file)
                    .and_then(|text| text.lines().nth(place.line.checked_sub(1)?))
                    .unwrap_or_default()
                    .to_owned()
            });
            Related {
                file,
                line: place.line,
                columns: place.columns.clone(),
                line_text,
                message,
            }
        };
        let file_of = |name: &str| {
            self.operation(name)
                .and_then(|operation| operation.file.clone())
        };
        let mut places: Vec<Related> = chain
            .windows(2)
            .rev()
            .map(|pair| {
                let (call, parent) = (pair[0], pair[1]);
                related(
                    file_of(&parent.operation),
                    &call.place,
                    format!(
                        "the call of `{}` in the body of `{}`",
                        call.operation, parent.operation
                    ),
                )
            })
            .collect();
        if let Some(step) = step {
            places.push(related(
                file_of(&chain[0].operation),
                step,
                format!("the step in the body of `{}`", chain[0].operation),
            ));
        }
        let prefix = if places.is_empty() {
            String::new()
        } else {
            format!("in `{}`: ", render_call(root))
        };
        Some(InCall {
            place: part.unwrap_or_else(|| written.call()),
            prefix,
            related: places,
        })
    }
}
