//! Lower a parsed [`Syntax`] to a [`Pipeline`]: merge imports, check that
//! names are declared once and before they are used, infer the dimensions
//! of the products a flow step declares without them, and put every
//! product's dimensions in the pipeline's order.

use std::collections::BTreeMap;

use crate::imports::apply_import;
use crate::model::{
    Cardinality, CommandDef, CoverageRule, Exclusion, InputBinding, InputRules, Invocation,
    OperationDef, Pipeline, ProductDef, SourceInventory, StageDef,
};
use crate::order::{order_dimensions, Output};
use crate::parser::{
    parse_source_inventory, parse_syntax, split_document, without_bom, ExcludeLine, FlowStep, Kind,
    ParseError, ParseErrorKind, PathRule, Rule, SourceMap, Statement, StatementKind, Step, Syntax,
};
use crate::shape::{step_context, step_driver, BoundInput};
use crate::span::Place;
use crate::types::TypeExpr;

/// A pipeline under construction, the input rules its document declares
/// beside it, and where its declarations sit.
#[derive(Default)]
pub(crate) struct PipelineBuilder {
    pub(crate) pipeline: Pipeline,
    pub(crate) inputs: InputRules,
    pub(crate) lines: SourceMap,
    /// The `dimensions [...]` line, and where it is.
    dimension_order: Option<(Vec<String>, Place)>,
    /// Each product a step makes, and whether the step wrote its dimensions.
    outputs: BTreeMap<String, Output>,
    /// Where each `sidecars` header is.
    sidecar_places: BTreeMap<String, Place>,
}

impl PipelineBuilder {
    pub(crate) fn add_product(&mut self, product: ProductDef, place: Place) {
        self.lines.products.insert(product.name.clone(), place);
        self.pipeline.products.push(product);
    }

    /// Add `operation`, declared at `place` in `stage`, unless an operation
    /// of its name is already declared or imported.
    pub(crate) fn add_operation(
        &mut self,
        operation: OperationDef,
        place: Place,
        stage: Option<&str>,
    ) -> Result<(), ParseError> {
        if let Some(first) = self.lines.operations.get(&operation.name) {
            let name = &operation.name;
            let earlier = if self.lines.imported.contains(name) {
                format!("the `use` on line {} imports one", first.line)
            } else {
                format!("it is already declared on line {}", first.line)
            };
            let global = if stage.is_some() || self.lines.operation_stages.contains_key(name) {
                "; operations are global even when declared in a stage, so give this one another name"
            } else {
                ""
            };
            let message = format!("duplicate operation `{name}`: {earlier}{global}");
            return Err(ParseError::new(place.line, message).within(&place));
        }
        if let Some(stage) = stage {
            self.lines
                .operation_stages
                .insert(operation.name.clone(), stage.to_owned());
        }
        self.lines.operations.insert(operation.name.clone(), place);
        self.pipeline.operations.push(operation);
        Ok(())
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

    fn add_invocation(&mut self, invocation: Invocation, step: &Step) {
        for output in &invocation.outputs {
            self.lines.invocations.insert(output.clone(), step.clone());
        }
        self.pipeline.invocations.push(invocation);
    }

    fn add_stage(&mut self, name: &str, place: Place) -> Result<(), ParseError> {
        if let Some(first) = self.lines.stages.get(name) {
            let message = format!(
                "duplicate stage `{name}`: it is already opened on line {}; a stage is one block, so move these lines into it",
                first.line
            );
            return Err(ParseError::new(place.line, message).within(&place));
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

    fn add_extension(
        &mut self,
        stage: Option<&str>,
        extension: &str,
        line: usize,
    ) -> Result<(), ParseError> {
        let (slot, whose) = match stage {
            Some(stage) => (
                &mut self
                    .pipeline
                    .stages
                    .iter_mut()
                    .find(|definition| definition.name == stage)
                    .expect("a stage is declared before its lines")
                    .extension,
                format!(" for stage `{stage}`"),
            ),
            None => (&mut self.pipeline.extension, String::new()),
        };
        if slot.replace(extension.to_owned()).is_some() {
            return Err(ParseError::new(line, format!("duplicate `ext:`{whose}")));
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
        let invocation = invocation.clone();
        let dimensions = inferred_dimensions(&invocation, operation, &self.pipeline);
        for (index, output) in outputs.iter().enumerate() {
            let declared = if output.dimensions.is_some() {
                Output::Annotated
            } else {
                Output::Inferred
            };
            self.outputs.insert(output.name.clone(), declared);
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
    if let Some(error) = &syntax.error {
        return Err(error.clone());
    }
    check_sidecar_paths(&builder)?;
    order_dimensions(
        &mut builder.pipeline.products,
        &builder.outputs,
        builder.dimension_order.as_ref(),
        &builder.lines,
    )?;
    Ok(builder)
}

/// A `sidecars` group's name is its own, and the group gives its members'
/// paths: by the `path:` line in its block, or else by the recipe. So no
/// product shares its name, and no `path` line outside the block names the
/// group or a member.
fn check_sidecar_paths(builder: &PipelineBuilder) -> Result<(), ParseError> {
    let PipelineBuilder {
        pipeline,
        lines,
        sidecar_places,
        ..
    } = builder;
    for group in &pipeline.sidecar_groups {
        let name = &group.name;
        if pipeline
            .products
            .iter()
            .any(|product| product.name == *name)
        {
            let place = &sidecar_places[name];
            return Err(ParseError::new(
                place.line,
                format!("sidecars group `{name}` shares its name with a product; rename one, since `path {name}:` in a recipe must name one thing"),
            )
            .within(place));
        }
        if let Some(place) = lines.paths.get(name) {
            return Err(ParseError::new(
                place.line,
                format!("sidecars group `{name}` gives its stem on an indented `path:` line in its block"),
            )
            .within(place));
        }
        if group.stem.is_none() {
            for (member, _) in &group.members {
                if let Some(place) = lines.paths.get(member) {
                    return Err(ParseError::new(
                        place.line,
                        format!("source `{member}` takes its path from sidecars group `{name}`; give the group's stem on an indented `path:` line in its block"),
                    )
                    .within(place));
                }
            }
        }
    }
    Ok(())
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
        StatementKind::Operation(operation, place, stage) => {
            builder.add_operation(operation.clone(), place.clone(), stage.as_deref())?;
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
        StatementKind::Dimensions(order) => {
            if builder.dimension_order.is_some() {
                return Err(ParseError::new(
                    statement.place.line,
                    "a pipeline has one `dimensions` line",
                ));
            }
            builder.dimension_order = Some((order.clone(), statement.place.clone()));
        }
        StatementKind::Path(rule) => builder.add_path(rule, statement.place.line)?,
        StatementKind::SidecarGroup(group) => {
            if builder
                .pipeline
                .sidecar_groups
                .iter()
                .any(|existing| existing.name == group.name)
            {
                return Err(ParseError::new(
                    statement.place.line,
                    format!("duplicate sidecars group `{}`", group.name),
                ));
            }
            builder
                .sidecar_places
                .insert(group.name.clone(), statement.place.clone());
            builder.pipeline.sidecar_groups.push(group.clone());
        }
        StatementKind::Extension { stage, extension } => {
            builder.add_extension(stage.as_deref(), extension, statement.place.line)?;
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
