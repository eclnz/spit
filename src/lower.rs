//! Lower a parsed [`Syntax`] to a [`Pipeline`]: merge imports, check that
//! names are declared once and before they are used, and infer the
//! dimensions of the products a flow step declares without them.

use std::collections::BTreeMap;

use crate::imports::apply_import;
use crate::model::{
    Cardinality, CommandDef, CoverageRule, Exclusion, InputBinding, InputRules, Invocation,
    OperationDef, Pipeline, ProductDef, SourceInventory, StageDef,
};
use crate::parser::{
    parse_source_inventory, parse_syntax, split_document, without_bom, ExcludeLine, FlowStep, Kind,
    ParseError, ParseErrorKind, PathRule, Rule, SourceMap, Statement, StatementKind, Step, Syntax,
};
use crate::shape::{effective_binding, step_context, step_driver, BoundInput};
use crate::span::Place;
use crate::types::TypeExpr;

/// A pipeline under construction, the input rules its document declares
/// beside it, and where its declarations sit.
#[derive(Default)]
pub(crate) struct PipelineBuilder {
    pub(crate) pipeline: Pipeline,
    pub(crate) inputs: InputRules,
    pub(crate) lines: SourceMap,
}

impl PipelineBuilder {
    pub(crate) fn add_product(&mut self, product: ProductDef, place: Place) {
        self.lines.products.insert(product.name.clone(), place);
        self.pipeline.products.push(product);
    }

    pub(crate) fn add_operation(&mut self, operation: OperationDef, place: Place) {
        self.lines.operations.insert(operation.name.clone(), place);
        self.pipeline.operations.push(operation);
    }

    pub(crate) fn add_constraint(&mut self, mut constraint: CoverageRule, rule: Rule) {
        constraint.line = Some(rule.line);
        self.lines
            .constraints
            .insert(constraint.product.clone(), rule.clone());
        self.lines.rules.push(rule);
        self.inputs.constraints.push(constraint);
    }

    pub(crate) fn add_command(&mut self, command: CommandDef, place: Place) {
        self.lines.commands.push(place);
        self.pipeline.commands.push(command);
    }

    fn add_invocation(&mut self, mut invocation: Invocation, step: &Step) {
        if let Some(operation) = self
            .pipeline
            .operations
            .iter()
            .find(|operation| operation.name == invocation.operation)
        {
            for (binding, port) in invocation.inputs.iter_mut().zip(&operation.inputs) {
                *binding = effective_binding(binding, port, operation);
            }
        }
        for output in &invocation.outputs {
            self.lines.invocations.insert(output.clone(), step.clone());
        }
        self.pipeline.invocations.push(invocation);
    }

    fn add_stage(&mut self, name: &str, place: Place) -> Result<(), ParseError> {
        if self.lines.stages.contains_key(name) {
            return Err(
                ParseError::new(place.line, format!("duplicate stage `{name}`")).within(&place),
            );
        }
        self.lines.stages.insert(name.to_owned(), place);
        self.pipeline.stages.push(StageDef::new(name));
        Ok(())
    }

    fn add_path(&mut self, rule: &PathRule, line: usize) -> Result<(), ParseError> {
        let Self {
            pipeline, lines, ..
        } = self;
        let template = rule.template.clone();
        if let Some(product) = &rule.product {
            lines.paths.insert(product.clone(), rule.place.clone());
            if pipeline
                .product_paths
                .insert(product.clone(), template)
                .is_some()
            {
                return Err(ParseError::new(
                    line,
                    format!("duplicate path template for product `{product}`"),
                ));
            }
        } else if let Some(stage) = &rule.stage {
            let definition = pipeline
                .stages
                .iter_mut()
                .find(|definition| &definition.name == stage)
                .expect("a stage is declared before its lines");
            if definition.path_template.replace(template).is_some() {
                return Err(ParseError::new(
                    line,
                    format!("duplicate default path template for stage `{stage}`"),
                ));
            }
            lines.stage_paths.insert(stage.clone(), rule.place.clone());
        } else if pipeline.path_template.replace(template).is_some() {
            return Err(ParseError::new(line, "duplicate default path template"));
        } else {
            lines.default_path = Some(rule.place.clone());
        }
        Ok(())
    }

    /// Add a flow step and the products it declares, inferring the
    /// dimensions of those declared without them.
    fn add_flow_step(&mut self, flow: &FlowStep) -> Result<(), ParseError> {
        let FlowStep {
            invocation,
            outputs,
            step,
        } = flow;
        let name = &invocation.operation;
        let operation = self
            .pipeline
            .operations
            .iter()
            .find(|operation| &operation.name == name)
            .ok_or_else(|| {
                let place = step.operation();
                ParseError::new(
                    place.line,
                    format!("operation `{name}` must be declared before its first flow step"),
                )
                .within(&place)
                .with_kind(ParseErrorKind::UndeclaredOperation { name: name.clone() })
            })?;
        let mut invocation = invocation.clone();
        for (binding, port) in invocation.inputs.iter_mut().zip(&operation.inputs) {
            *binding = effective_binding(binding, port, operation);
        }
        let dimensions = inferred_dimensions(&invocation, operation, &self.pipeline);
        for (index, output) in outputs.iter().enumerate() {
            let product = ProductDef::new(
                output.name.clone(),
                output.artifact_type.clone().unwrap_or(TypeExpr::Unknown),
                output
                    .dimensions
                    .clone()
                    .unwrap_or_else(|| dimensions.clone()),
            );
            self.add_product(product, step.output_at(index));
        }
        self.add_invocation(invocation, step);
        Ok(())
    }
}

/// Lower each statement in order, with `imports` holding the definitions
/// each `use` line brings in, by line. The first error by line wins: one in
/// a statement, or else the syntax error parsing stopped at.
pub(crate) fn lower(
    syntax: &Syntax,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> Result<PipelineBuilder, ParseError> {
    let mut builder = PipelineBuilder::default();
    for statement in &syntax.statements {
        let rule = matches!(
            statement.kind,
            StatementKind::Discover(_) | StatementKind::Constraint(..) | StatementKind::Exclude(..)
        );
        if rule && kind == Kind::Pipeline {
            return Err(ParseError::new(
                statement.place.line,
                "`discover`, `require`, `drop` and `exclude` rules belong in a .spitin recipe, not a pipeline",
            )
            .within(&statement.place));
        }
        lower_statement(&mut builder, imports, statement)
            .map_err(|error| error.within(&statement.place))?;
    }
    match &syntax.error {
        Some(error) => Err(error.clone()),
        None => Ok(builder),
    }
}

fn lower_statement(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    statement: &Statement,
) -> Result<(), ParseError> {
    match &statement.kind {
        StatementKind::Import => apply_import(builder, imports, &statement.place)?,
        StatementKind::Stage { name, place } => builder.add_stage(name, place.clone())?,
        StatementKind::Product(product, place) => {
            builder.add_product(product.clone(), place.clone());
        }
        StatementKind::Discover(discovery) => {
            if builder.inputs.discovery(&discovery.name).is_some() {
                return Err(ParseError::new(
                    statement.place.line,
                    format!("duplicate discovery `{}`", discovery.name),
                ));
            }
            builder.inputs.discoveries.push(discovery.clone());
        }
        StatementKind::Operation(operation, place) => {
            builder.add_operation(operation.clone(), place.clone());
        }
        StatementKind::Constraint(constraint, rule) => {
            builder.add_constraint(constraint.clone(), rule.clone());
        }
        StatementKind::Exclude(ExcludeLine::File(file), ..) => {
            builder
                .inputs
                .exclusion_files
                .push((file.clone(), statement.place.line));
        }
        StatementKind::Exclude(ExcludeLine::Rule { product, values }, reason, place) => {
            builder.inputs.exclusions.push(Exclusion {
                product: product.clone(),
                values: values.clone(),
                reason: reason.clone(),
                origin: format!("line {}", statement.place.line),
            });
            builder.lines.exclusions.push(place.clone());
        }
        StatementKind::Command(command, place) => {
            builder.add_command(command.clone(), place.clone());
        }
        StatementKind::Path(rule) => builder.add_path(rule, statement.place.line)?,
        StatementKind::Step(invocation, step) => {
            builder.add_invocation(invocation.clone(), step);
        }
        StatementKind::FlowStep(flow) => builder.add_flow_step(flow)?,
    }
    Ok(())
}

/// The dimensions a flow step's undeclared outputs take: those of the input
/// that drives it, less a varied or pinned dimension.
fn inferred_dimensions(
    invocation: &Invocation,
    operation: &OperationDef,
    pipeline: &Pipeline,
) -> Vec<String> {
    let dimensions = |binding: &InputBinding| {
        pipeline
            .products
            .iter()
            .find(|product| product.name == binding.product)
            .map(|product| binding.free_dimensions(&product.dimensions))
    };
    let inputs: Option<Vec<_>> = invocation
        .inputs
        .iter()
        .zip(&operation.inputs)
        .map(|(binding, port)| {
            Some(BoundInput {
                binding,
                dimensions: dimensions(binding)?,
                many: port.cardinality == Cardinality::Many,
            })
        })
        .collect();
    // Otherwise the step is invalid; the resolver reports why.
    inputs
        .filter(|inputs| inputs.len() == invocation.inputs.len())
        .and_then(|inputs| {
            let (_, groups) = step_driver(&inputs)?;
            Some(step_context(&inputs, &groups))
        })
        .or_else(|| invocation.inputs.first().and_then(dimensions))
        .unwrap_or_default()
}

/// A parsed document: its pipeline, or a recipe's input rules and records,
/// and declaration lines.
pub(crate) struct ParsedDocument {
    pub(crate) pipeline: Pipeline,
    pub(crate) inputs: InputRules,
    pub(crate) inventory: Option<SourceInventory>,
    pub(crate) lines: SourceMap,
}

/// Parse a pipeline. Input rules and records are not part of one: they
/// belong in a `.spitin` recipe and a `.spitout`.
pub fn parse_pipeline(text: &str) -> Result<Pipeline, ParseError> {
    parse_document_with_imports(without_bom(text), &BTreeMap::new(), Kind::Pipeline)
        .map(|document| document.pipeline)
}

pub(crate) fn parse_document_with_imports(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> Result<ParsedDocument, ParseError> {
    let document = split_document(text);
    let lowered = lower(&parse_syntax(&document.pipeline), imports, kind);
    let inventory = match (kind, document.inventory_line) {
        (_, None) => None,
        (Kind::Recipe, Some(_)) => Some(parse_source_inventory(&document.inventory)?),
        // An error on an earlier line is the first.
        (Kind::Pipeline, Some(line)) => {
            let earlier = lowered
                .as_ref()
                .err()
                .is_some_and(|error| error.line() < line);
            if !earlier {
                let records = document
                    .inventory
                    .lines()
                    .enumerate()
                    .filter(|(_, text)| !text.trim().is_empty())
                    .map(|(index, _)| index + 1)
                    .collect();
                return Err(ParseError::new(
                    line,
                    "`sources:` and `contexts:` records belong in a .spitout, not a pipeline",
                )
                .with_kind(ParseErrorKind::MisplacedRecords { lines: records }));
            }
            None
        }
    };
    let builder = lowered?;
    Ok(ParsedDocument {
        pipeline: builder.pipeline,
        inputs: builder.inputs,
        inventory,
        lines: builder.lines,
    })
}
