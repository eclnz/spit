//! Lower a parsed [`Syntax`] to a [`Pipeline`]: merge imports, check that
//! names are declared once and before they are used, infer the dimensions
//! of the products a flow step declares without them, and put every
//! product's dimensions in the pipeline's order.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

mod expand;

use crate::imports::apply_import;
use crate::model::{
    CallId, Cardinality, CheckDef, CommandDef, CommandRole, CoverageRule, DefaultChecks, Exclusion,
    InputBinding, InputRules, Invocation, OperationDef, Pipeline, ProductDef, ProductId,
    SidecarGroup, SourceInventory, StageDef, StepOutput,
};
use crate::order::{order_dimensions, Output};
use crate::parser::{
    parse_source_inventory, parse_syntax, split_document, without_bom, ExcludeLine, Kind,
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
    /// Each product's id, the first of a name, so a step finds its inputs
    /// without searching every product.
    product_ids: FxHashMap<String, ProductId>,
    /// Each main source with companions, found once when the first
    /// companion is declared. Keep in step with `add_sidecar_group`.
    sidecar_group_numbers: FxHashMap<String, usize>,
    /// The position in `pipeline.operations` of each operation by name, the
    /// first of a name, so a step finds what it calls without searching
    /// every operation. Keep in step with `add_operation`, the only place
    /// that adds one.
    operation_at: FxHashMap<String, usize>,
    /// Each product a call made for itself, with the call and the operation
    /// it calls, which steps written in the pipeline may not read.
    intermediates: FxHashMap<String, CallId>,
}

impl PipelineBuilder {
    pub(crate) fn add_product(&mut self, product: ProductDef, place: Place) {
        self.lines.products.insert(product.name.clone(), place);
        self.product_ids
            .entry(product.name.clone())
            .or_insert(ProductId::at(self.pipeline.products.len()));
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
        self.operation_at
            .insert(operation.name.clone(), self.pipeline.operations.len());
        self.pipeline.operations.push(operation);
        Ok(())
    }

    /// Keep the members of one source's `beside` declarations together for
    /// missing-companion reporting and imports.
    pub(crate) fn add_sidecar_group(&mut self, group: SidecarGroup) {
        self.sidecar_group_numbers
            .insert(group.name.clone(), self.pipeline.sidecar_groups.len());
        self.pipeline.sidecar_groups.push(group);
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

    pub(crate) fn add_check(&mut self, check: CheckDef, place: Place) {
        self.lines.checks.push(place);
        self.pipeline.checks.push(check);
    }

    fn add_invocation(&mut self, invocation: Invocation, step: &Step) {
        for output in &invocation.outputs {
            self.lines.invocations.insert(output.clone(), step.clone());
        }
        self.pipeline.invocations.push(invocation);
    }

    /// Open stage `name`. A stage opened again continues where its first
    /// block left off: its steps join the same stage, and the source map keeps
    /// the first header as the stage's place.
    fn add_stage(&mut self, name: &str, place: Place) {
        if self.lines.stages.contains_key(name) {
            return;
        }
        self.lines.stages.insert(name.to_owned(), place);
        self.pipeline.stages.push(StageDef::new(name));
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

    fn add_default_checks(
        &mut self,
        stage: Option<&str>,
        checks: &DefaultChecks,
        place: &Place,
    ) -> Result<(), ParseError> {
        let Self {
            pipeline, lines, ..
        } = self;
        let (slot, placed, whose) = match stage {
            Some(stage) => (
                &mut pipeline
                    .stages
                    .iter_mut()
                    .find(|definition| definition.name == stage)
                    .expect("a stage is declared before its lines")
                    .checks,
                lines.stage_checks.entry(stage.to_owned()).or_default(),
                format!(" for stage `{stage}`"),
            ),
            None => (
                &mut pipeline.default_checks,
                lines.default_checks.get_or_insert_with(Place::default),
                String::new(),
            ),
        };
        if *slot != DefaultChecks::default() {
            return Err(ParseError::new(
                place.line,
                format!("duplicate `check:` list{whose}; write the checks in one line"),
            )
            .within(place));
        }
        *slot = checks.clone();
        *placed = place.clone();
        Ok(())
    }

    /// Add one step and the products it declares, inferring the dimensions
    /// of those declared without them. The operation it calls is declared,
    /// and is carried out by a command. `step` is where the step written in
    /// the pipeline is, naming `written`: for a step a call made, the call.
    fn add_step(
        &mut self,
        invocation: Invocation,
        outputs: &[StepOutput],
        step: &Step,
        written: &[String],
    ) {
        let operation = &self.pipeline.operations[self.operation_at[&invocation.operation]];
        let dimensions = inferred_dimensions(
            &invocation,
            operation,
            &self.pipeline.products,
            &self.product_ids,
        );
        for output in outputs {
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
            // A product the call made for itself is placed at the call.
            let place = written
                .iter()
                .position(|name| *name == output.name)
                .map_or_else(|| step.call(), |index| step.output_at(index));
            self.add_product(product, place);
        }
        self.add_invocation(invocation, step);
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
    check_bodies_have_no_commands(&builder)?;
    check_source_beside_paths(&builder)?;
    order_dimensions(
        &mut builder.pipeline.products,
        &builder.outputs,
        builder.dimension_order.as_ref(),
        &builder.lines,
    )?;
    Ok(builder)
}

/// An operation with a body is carried out by its steps, so it takes no
/// `command` or `verify` line of its own.
fn check_bodies_have_no_commands(builder: &PipelineBuilder) -> Result<(), ParseError> {
    let PipelineBuilder {
        pipeline,
        lines,
        operation_at,
        ..
    } = builder;
    for (command, place) in pipeline.commands.iter().zip(&lines.commands) {
        let Some(&operation) = operation_at.get(&command.operation) else {
            continue;
        };
        if !pipeline.operations[operation].steps.is_empty() {
            return Err(ParseError::new(
                place.line,
                format!(
                    "operation `{}` is carried out by the steps in its body, so it takes no `{}` line; give each step's operation its own",
                    command.operation,
                    match command.role {
                        CommandRole::Run => "command",
                        CommandRole::Verify => "verify",
                    }
                ),
            )
            .within(place));
        }
    }
    Ok(())
}

/// A source written beside another takes the other's path, so it has no
/// independent path rule in the pipeline.
fn check_source_beside_paths(builder: &PipelineBuilder) -> Result<(), ParseError> {
    for product in &builder.pipeline.products {
        let Some(beside) = &product.beside else {
            continue;
        };
        if let Some(place) = builder.lines.paths.get(&product.name) {
            return Err(ParseError::new(
                place.line,
                format!("source `{}` is beside `{}`, so its path follows that source; remove its path rule", product.name, beside.sibling),
            )
            .within(place));
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
        StatementKind::Stage { name, place } => builder.add_stage(name, place.clone()),
        StatementKind::Product(product, place) => {
            let mut product = product.clone();
            if let Some(beside) = &product.beside {
                let sibling = &beside.sibling;
                let anchor = builder
                    .product_ids
                    .get(sibling)
                    .and_then(|&id| builder.pipeline.products.get(id.index()))
                    .ok_or_else(|| {
                        ParseError::new(
                            place.line,
                            format!("source `{}` is beside unknown source `{sibling}`; declare `{sibling}` first", product.name),
                        )
                    })?;
                if anchor.beside.is_some()
                    || anchor.folder
                    || anchor.extension.is_none()
                    || builder.outputs.contains_key(sibling)
                {
                    return Err(ParseError::new(
                        place.line,
                        format!("source `{}` must be beside a file source with an extension, not `{sibling}`", product.name),
                    ));
                }
                product.dimensions.clone_from(&anchor.dimensions);
                if let Some(&number) = builder.sidecar_group_numbers.get(sibling) {
                    let group = builder
                        .pipeline
                        .sidecar_groups
                        .get_mut(number)
                        .expect("a companion group index is recorded when the group is added");
                    group
                        .members
                        .push((product.name.clone(), beside.suffix.clone()));
                } else {
                    let extension = anchor.extension.as_ref().expect("checked above");
                    builder.add_sidecar_group(SidecarGroup {
                        name: sibling.clone(),
                        dimensions: anchor.dimensions.clone(),
                        members: vec![
                            (sibling.clone(), extension.clone()),
                            (product.name.clone(), beside.suffix.clone()),
                        ],
                    });
                }
            }
            builder.add_product(product, place.clone());
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
            if !operation.steps.is_empty() {
                builder.check_body(operation, place)?;
            }
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
        StatementKind::Check(check, place) => {
            builder.add_check(check.clone(), place.clone());
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
        StatementKind::DefaultChecks {
            stage,
            checks,
            place,
        } => builder.add_default_checks(stage.as_deref(), checks, place)?,
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
    products: &[ProductDef],
    product_ids: &FxHashMap<String, ProductId>,
) -> Vec<String> {
    let dimensions = |binding: &InputBinding| {
        product_ids
            .get(&binding.product)
            .map(|id| binding.free_dimensions(&products[id.index()].dimensions))
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
