//! Lower a parsed [`Syntax`] to a [`Pipeline`]: merge imports, check that
//! names are declared once and before they are used, infer the dimensions
//! of the products a flow step declares without them, and put every
//! product's dimensions in the pipeline's order.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

mod entities;
mod expand;

use expand::BodyCheck;

use crate::imports::apply_import;
use crate::model::{
    CallId, Cardinality, CheckDef, CommandDef, CommandRole, CoverageRule, DefaultChecks, Exclusion,
    InputBinding, InputRules, Invocation, OperationDef, Pipeline, ProductDef, ProductId,
    ResourcesDef, SidecarGroup, SourceInventory, StageDef, StepOutput,
};
use crate::order::{order_dimensions, Output};
use crate::parser::{
    document_records, empty_body, parse_syntax, without_bom, CommandPlaces, ExcludeLine, Kind,
    Layout, ParseError, ParseErrorKind, PathRule, Rule, SourceMap, Statement, StatementKind, Step,
    Syntax,
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
    /// Whether a `types` line has been read.
    types_line: bool,
    entities: entities::EntitiesDeclarations,
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
    /// While a flow step is added, what each map entry it wrote held
    /// before, so that a step that fails leaves the builder as it was
    /// (`add_flow_step`). Keep in step with every map a step writes.
    journal: Option<Vec<Undo>>,
}

/// A map entry a flow step wrote, and what it held before.
enum Undo {
    Output(String, Option<Output>),
    ProductPlace(String, Option<Place>),
    ProductId(String),
    Invocation(String, Option<Step>),
    Intermediate(String, Option<CallId>),
}

impl PipelineBuilder {
    pub(crate) fn add_product(&mut self, product: ProductDef, place: Place) {
        let before = self.lines.products.insert(product.name.clone(), place);
        self.record(|| Undo::ProductPlace(product.name.clone(), before));
        if !self.product_ids.contains_key(&product.name) {
            self.product_ids.insert(
                product.name.clone(),
                ProductId::at(self.pipeline.products.len()),
            );
            self.record(|| Undo::ProductId(product.name.clone()));
        }
        self.pipeline.products.push(product);
    }

    /// Note `undo` while a flow step is added.
    fn record(&mut self, undo: impl FnOnce() -> Undo) {
        if let Some(journal) = &mut self.journal {
            journal.push(undo());
        }
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

    /// Add `command`, which is at `places`.
    pub(crate) fn add_command(&mut self, command: CommandDef, places: CommandPlaces) {
        self.lines.commands.push(places);
        self.pipeline.commands.push(command);
    }

    pub(crate) fn add_resources(&mut self, resources: ResourcesDef, place: Place) {
        self.lines.resources.push(place);
        self.pipeline.resources.push(resources);
    }

    pub(crate) fn add_check(&mut self, check: CheckDef, place: Place) {
        self.lines.checks.push(place);
        self.pipeline.checks.push(check);
    }

    fn add_invocation(&mut self, invocation: Invocation, step: &Step) {
        for output in &invocation.outputs {
            let before = self.lines.invocations.insert(output.clone(), step.clone());
            self.record(|| Undo::Invocation(output.clone(), before));
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

    /// Add a path rule. A rule that repeats one already given fails before
    /// it changes anything.
    fn add_path(&mut self, rule: &PathRule, line: usize) -> Result<(), ParseError> {
        let Self {
            pipeline, lines, ..
        } = self;
        let template = rule.template.clone();
        if let Some(product) = &rule.product {
            if pipeline.product_paths.contains_key(product) {
                return Err(ParseError::new(
                    line,
                    format!("duplicate path template for product `{product}`"),
                ));
            }
            lines.paths.insert(product.clone(), rule.place.clone());
            pipeline.product_paths.insert(product.clone(), template);
        } else if let Some(stage) = &rule.stage {
            let definition = pipeline
                .stages
                .iter_mut()
                .find(|definition| &definition.name == stage)
                .expect("a stage is declared before its lines");
            if definition.path_template.is_some() {
                return Err(ParseError::new(
                    line,
                    format!("duplicate default path template for stage `{stage}`"),
                ));
            }
            definition.path_template = Some(template);
            lines.stage_paths.insert(stage.clone(), rule.place.clone());
        } else if pipeline.path_template.is_some() {
            return Err(ParseError::new(line, "duplicate default path template"));
        } else {
            pipeline.path_template = Some(template);
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
        if slot.is_some() {
            return Err(ParseError::new(
                line,
                format!("duplicate `default ext:`{whose}"),
            ));
        }
        *slot = Some(extension.to_owned());
        Ok(())
    }

    /// Add a `check:` list. One that repeats the list already given fails
    /// before it changes anything.
    fn add_default_checks(
        &mut self,
        stage: Option<&str>,
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
                format!(" for stage `{stage}`"),
            ),
            None => (&mut pipeline.default_checks, String::new()),
        };
        if *slot != DefaultChecks::default() {
            return Err(ParseError::new(
                place.line,
                format!("duplicate `default check:` list{whose}; write the checks in one line"),
            )
            .within(place));
        }
        *slot = checks.clone();
        let placed = match stage {
            Some(stage) => lines.stage_checks.entry(stage.to_owned()).or_default(),
            None => lines.default_checks.get_or_insert_with(Place::default),
        };
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
            let before = self.outputs.insert(output.name.clone(), declared);
            self.record(|| Undo::Output(output.name.clone(), before));
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
/// each `use` line brings in, by line. The errors are the syntax errors
/// parsing found and the statements that fail, in the order they were read.
/// A statement is lowered whole or not at all, so lowering goes on past
/// every one that fails, and the builder it gives holds every statement
/// that did not.
pub(crate) fn lower(
    syntax: &Syntax,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> (PipelineBuilder, Vec<ParseError>) {
    let mut builder = PipelineBuilder::default();
    let mut errors = Vec::new();
    let mut parsing = syntax.errors.iter().peekable();
    for (index, statement) in syntax.statements.iter().enumerate() {
        while let Some((_, error)) = parsing.next_if(|(read, _)| *read <= index) {
            errors.push(error.clone());
        }
        let rule = matches!(
            statement.kind,
            StatementKind::Constraint(..) | StatementKind::Exclude(..)
        );
        let lowered = if rule && kind == Kind::Pipeline {
            Err(ParseError::new(
                statement.place.line,
                "`require` and `exclude` rules belong in a .spitin recipe, not a pipeline",
            ))
        } else {
            lower_statement(&mut builder, imports, statement, &mut errors)
        };
        // A statement that fails leaves the builder as it was, so lowering
        // goes on, and the statements after it read as they would without it.
        if let Err(error) = lowered {
            errors.push(error.within(&statement.place));
        }
    }
    errors.extend(parsing.map(|(_, error)| error.clone()));
    // The checks of the whole pipeline read what was lowered, without the
    // statements that failed, as the pipeline would read once they are
    // fixed or gone. Each gives its first error.
    let checks = [
        check_bodies_have_no_commands(&builder),
        check_source_beside_paths(&builder),
        order_dimensions(
            &mut builder.pipeline.products,
            &builder.outputs,
            builder.dimension_order.as_ref(),
            &builder.lines,
        ),
        builder.entities.validate(&mut builder.pipeline),
    ];
    errors.extend(checks.into_iter().filter_map(Result::err));
    (builder, errors)
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
    for (command, places) in pipeline.commands.iter().zip(&lines.commands) {
        let Some(&operation) = operation_at.get(&command.operation) else {
            continue;
        };
        let place = &places.header;
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

/// Lower `statement`. The errors of steps in the body of an operation, which
/// come before the statement's own, go to `errors`.
fn lower_statement(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    statement: &Statement,
    errors: &mut Vec<ParseError>,
) -> Result<(), ParseError> {
    match &statement.kind {
        StatementKind::Import => apply_import(builder, imports, &statement.place)?,
        StatementKind::Root(root) => {
            if builder.pipeline.root.is_some() {
                return Err(ParseError::new(
                    statement.place.line,
                    "a .spit names its root once",
                ));
            }
            builder.pipeline.root = Some((root.clone(), statement.place.line));
        }
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
                    || (anchor.extension.is_none() && !beside.append)
                    || builder.outputs.contains_key(sibling)
                {
                    let what = if beside.append {
                        "a file source"
                    } else {
                        "a file source with an extension, or `+` to append to the whole name,"
                    };
                    return Err(ParseError::new(
                        place.line,
                        format!(
                            "source `{}` must be beside {what} not `{sibling}`",
                            product.name
                        ),
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
                    // A main source with no extension of its own is named
                    // by its source when a companion is missing.
                    let extension = anchor.extension.clone().unwrap_or_else(|| sibling.clone());
                    builder.add_sidecar_group(SidecarGroup {
                        name: sibling.clone(),
                        dimensions: anchor.dimensions.clone(),
                        members: vec![
                            (sibling.clone(), extension),
                            (product.name.clone(), beside.suffix.clone()),
                        ],
                    });
                }
            }
            builder.add_product(product, place.clone());
        }
        StatementKind::Operation(operation, place, stage, _) => {
            if operation.steps.is_empty() {
                return builder.add_operation(operation.clone(), place.clone(), stage.as_deref());
            }
            lower_body(builder, statement, errors)?;
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
        StatementKind::Command(command, places) => {
            builder.add_command(command.clone(), places.clone());
        }
        StatementKind::Resources(resources, place) => {
            builder.add_resources(resources.clone(), place.clone());
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
        StatementKind::TypeDecl(name, params) => {
            if builder
                .pipeline
                .type_decls
                .get(name)
                .is_some_and(|earlier| earlier != params)
            {
                return Err(ParseError::new(
                    statement.place.line,
                    format!("type `{name}` is already declared with other parameters"),
                ));
            }
            builder
                .pipeline
                .type_decls
                .insert(name.clone(), params.clone());
        }
        StatementKind::Types(types) => {
            if builder.types_line {
                return Err(ParseError::new(
                    statement.place.line,
                    "a pipeline has one `types` line",
                ));
            }
            builder.types_line = true;
            builder.pipeline.types = *types;
        }
        StatementKind::Entities { stage, declaration } => builder.entities.add(
            &mut builder.pipeline,
            stage.as_deref(),
            declaration,
            &statement.place,
        )?,
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

/// Lower `statement`, an operation with a body. A step of the body that
/// fails to check has its error given to `errors` and is left out, and the
/// steps after it are checked without it, so that a body with many bad
/// steps is checked once. The operation is added with the steps that are
/// left, and a body left with none is an error on the header. Every
/// failure comes before the builder changes.
fn lower_body(
    builder: &mut PipelineBuilder,
    statement: &Statement,
    errors: &mut Vec<ParseError>,
) -> Result<(), ParseError> {
    let StatementKind::Operation(operation, place, stage, ended) = &statement.kind else {
        unreachable!("an operation is lowered from an operation statement");
    };
    let BodyCheck { failed, unmade } = builder.check_body(operation, place)?;
    let all_failed = failed.len() == operation.steps.len();
    let mut kept = operation.clone();
    if !failed.is_empty() {
        let mut position = 0;
        let mut left_out = failed.iter().map(|(step, _)| *step).peekable();
        kept.steps.retain(|_| {
            position += 1;
            left_out.next_if_eq(&(position - 1)).is_none()
        });
    }
    errors.extend(failed.into_iter().map(|(_, error)| error));
    if all_failed {
        let line = statement.place.line;
        errors.push(empty_body(&operation.name, line).within(&Place::new(line, ended.clone())));
        return Ok(());
    }
    if let Some(error) = unmade {
        return Err(error);
    }
    builder.add_operation(kept, place.clone(), stage.as_deref())
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
    match parse_document_recovering(text, imports, kind) {
        (document, errors) if errors.is_empty() => Ok(document),
        (_, errors) => Err(errors
            .into_iter()
            .next()
            .expect("a document that fails has an error")),
    }
}

/// As [`parse_document_with_imports`], in one parse: the document, without
/// the statements that fail, and every error, the first being the error the
/// other gives.
pub(crate) fn parse_document_recovering(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> (ParsedDocument, Vec<ParseError>) {
    parse_layout(&Layout::new(text), imports, kind)
}

/// As [`parse_document_recovering`], for the document laid out in
/// `layout`.
pub(crate) fn parse_layout(
    layout: &Layout,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> (ParsedDocument, Vec<ParseError>) {
    let (builder, mut errors) = lower(&parse_syntax(layout), imports, kind);
    // The line of the first `sources:` or `contexts:` header.
    let records_line = layout
        .numbered()
        .find(|(_, line)| line.record)
        .map(|(number, _)| number);
    let inventory = match (kind, records_line) {
        (_, None) => None,
        (Kind::Recipe, Some(_)) => {
            let (inventory, found) = document_records(layout);
            errors.extend(found);
            Some(inventory)
        }
        (Kind::Pipeline, Some(line)) => {
            let records = layout
                .numbered()
                .filter(|(_, read)| read.record && !read.text.trim().is_empty())
                .map(|(number, _)| number)
                .collect();
            // In line order, as a caller that takes the first error reads
            // the first in the document.
            let at = errors
                .iter()
                .take_while(|error| error.line() < line)
                .count();
            errors.insert(
                at,
                ParseError::new(
                    line,
                    "`sources:` and `contexts:` records belong in a .spitout, not a pipeline",
                )
                .with_kind(ParseErrorKind::MisplacedRecords { lines: records }),
            );
            None
        }
    };
    let document = ParsedDocument {
        pipeline: builder.pipeline,
        inputs: builder.inputs,
        inventory,
        lines: builder.lines,
    };
    (document, errors)
}
