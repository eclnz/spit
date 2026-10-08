//! Lower a parsed [`Syntax`] to a [`Pipeline`]: merge imports, check that
//! names are declared once and before they are used, infer the dimensions
//! of the products a flow step declares without them, and put every
//! product's dimensions in the pipeline's order.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

mod expand;

use expand::BodyCheck;

use crate::imports::apply_import;
use crate::model::{
    CallId, Cardinality, CheckDef, CommandDef, CommandRole, CoverageRule, DefaultChecks, Exclusion,
    InputBinding, InputRules, Invocation, OperationDef, Pipeline, ProductDef, ProductId,
    SidecarGroup, SourceInventory, StageDef, StepOutput,
};
use crate::order::{order_dimensions, Output};
use crate::parser::{
    empty_body, parse_source_inventory, parse_syntax, split_document, without_bom, ExcludeLine,
    Kind, ParseError, ParseErrorKind, PathRule, Rule, SourceMap, Statement, StatementKind, Step,
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

    /// Add a path rule. A rule that repeats one already given fails before
    /// it changes anything, as a blank line would leave the builder.
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
            return Err(ParseError::new(line, format!("duplicate `ext:`{whose}")));
        }
        *slot = Some(extension.to_owned());
        Ok(())
    }

    /// Add a `check:` list. One that repeats the list already given fails
    /// before it changes anything, as a blank line would leave the builder.
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
                format!("duplicate `check:` list{whose}; write the checks in one line"),
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

/// A statement that did not lower. Keep in step with `parse_flow` in
/// `src/parser/flow.rs`, whose lines change what later lines mean only once
/// they succeed: a statement that may be passed over as a blank line is one
/// that did not change the builder and is not `stateful`.
pub(crate) struct Failure {
    /// Boxed, so that `Result` stays small where a statement succeeds.
    pub(crate) error: Box<ParseError>,
    /// It left the builder as it was before the statement, as a blank line
    /// would, and its error is on the statement's own line.
    pub(crate) clean: bool,
}

impl Failure {
    pub(crate) fn clean(error: ParseError) -> Self {
        Self {
            error: Box::new(error),
            clean: true,
        }
    }
}

/// An error from a statement that may have changed the builder before it
/// failed, unless it says it did not.
impl From<ParseError> for Failure {
    fn from(error: ParseError) -> Self {
        Self {
            error: Box::new(error),
            clean: false,
        }
    }
}

/// Lower each statement in order, with `imports` holding the definitions
/// each `use` line brings in, by line. The errors are those that blanking
/// the line of the first, and then of the next, would find, one after the
/// other: the syntax errors parsing found, and the statements that fail,
/// in the order they were read. Lowering goes on past a statement that
/// failed without changing the builder, and stops at one that may have, as
/// blanking its line would not leave the later statements reading the same.
pub(crate) fn lower(
    syntax: &Syntax,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> Result<PipelineBuilder, Vec<ParseError>> {
    let mut builder = PipelineBuilder::default();
    let mut errors = Vec::new();
    let mut parsing = syntax.errors.iter().peekable();
    let mut defer_parse_errors = false;
    for (index, statement) in syntax.statements.iter().enumerate() {
        while let Some((_, error)) = parsing.next_if(|(read, _)| *read <= index) {
            if !defer_parse_errors {
                errors.push(error.clone());
            }
        }
        let rule = matches!(
            statement.kind,
            StatementKind::Discover(_) | StatementKind::Constraint(..) | StatementKind::Exclude(..)
        );
        let lowered = if rule && kind == Kind::Pipeline {
            Err(Failure::clean(ParseError::new(
                statement.place.line,
                "`discover`, `require` and `exclude` rules belong in a .spitin recipe, not a pipeline",
            )))
        } else {
            lower_statement(&mut builder, imports, statement, &mut errors)
        };
        if let Err(failure) = lowered {
            let error = failure.error.within(&statement.place);
            // Diagnose later statements in the original parse even when
            // blanking this line would change their parse. Recovery will
            // blank the reported lines and parse that interpretation next.
            let goes_on = failure.clean && error.line() == statement.place.line;
            if goes_on && statement.stateful {
                // These syntax errors may disappear when recovery blanks the
                // failed line. Read them on the next pass instead.
                defer_parse_errors = true;
            }
            errors.push(error);
            if !goes_on {
                return Err(errors);
            }
        }
    }
    if !defer_parse_errors {
        errors.extend(parsing.map(|(_, error)| error.clone()));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    check_bodies_have_no_commands(&builder).map_err(|error| vec![error])?;
    check_source_beside_paths(&builder).map_err(|error| vec![error])?;
    order_dimensions(
        &mut builder.pipeline.products,
        &builder.outputs,
        builder.dimension_order.as_ref(),
        &builder.lines,
    )
    .map_err(|error| vec![error])?;
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

/// Lower `statement`. The errors of steps in the body of an operation, which
/// come before the statement's own, go to `errors`.
fn lower_statement(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    statement: &Statement,
    errors: &mut Vec<ParseError>,
) -> Result<(), Failure> {
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
                        Failure::clean(ParseError::new(
                            place.line,
                            format!("source `{}` is beside unknown source `{sibling}`; declare `{sibling}` first", product.name),
                        ))
                    })?;
                if anchor.beside.is_some()
                    || anchor.folder
                    || anchor.extension.is_none()
                    || builder.outputs.contains_key(sibling)
                {
                    return Err(Failure::clean(ParseError::new(
                        place.line,
                        format!("source `{}` must be beside a file source with an extension, not `{sibling}`", product.name),
                    )));
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
                return Err(Failure::clean(ParseError::new(
                    statement.place.line,
                    format!("duplicate discovery `{}`", discovery.name),
                )));
            }
            builder.inputs.discoveries.push(discovery.clone());
        }
        StatementKind::Operation(operation, place, stage, _) => {
            if operation.steps.is_empty() {
                return builder
                    .add_operation(operation.clone(), place.clone(), stage.as_deref())
                    .map_err(Failure::clean);
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
        StatementKind::Command(command, place) => {
            builder.add_command(command.clone(), place.clone());
        }
        StatementKind::Check(check, place) => {
            builder.add_check(check.clone(), place.clone());
        }
        StatementKind::Dimensions(order) => {
            if builder.dimension_order.is_some() {
                return Err(Failure::clean(ParseError::new(
                    statement.place.line,
                    "a pipeline has one `dimensions` line",
                )));
            }
            builder.dimension_order = Some((order.clone(), statement.place.clone()));
        }
        StatementKind::Path(rule) => builder
            .add_path(rule, statement.place.line)
            .map_err(Failure::clean)?,
        StatementKind::DefaultChecks {
            stage,
            checks,
            place,
        } => builder
            .add_default_checks(stage.as_deref(), checks, place)
            .map_err(Failure::clean)?,
        StatementKind::Extension { stage, extension } => {
            builder
                .add_extension(stage.as_deref(), extension, statement.place.line)
                .map_err(Failure::clean)?;
        }
        StatementKind::FlowStep(flow) => builder.add_flow_step(flow)?,
    }
    Ok(())
}

/// Lower `statement`, an operation with a body. A step of the body that
/// fails to check has its error given to `errors` and is left out, as
/// blanking its line would leave it, and the steps after it are checked
/// without it, so that a body with many bad steps is checked once. The
/// operation is added with the steps that are left, and a body left with
/// none is an error on the header, which is blank in the end.
fn lower_body(
    builder: &mut PipelineBuilder,
    statement: &Statement,
    errors: &mut Vec<ParseError>,
) -> Result<(), Failure> {
    let StatementKind::Operation(operation, place, stage, ended) = &statement.kind else {
        unreachable!("an operation is lowered from an operation statement");
    };
    let BodyCheck { mut failed, unmade } = builder
        .check_body(operation, place)
        .map_err(Failure::from)?;
    let all_failed = failed.len() == operation.steps.len();
    if all_failed && statement.stateful {
        // Were the header blank, the lines after it would not read the same.
        // The next parse reads the body with no steps.
        let (_, last) = failed.pop().expect("a body has a step");
        errors.extend(failed.into_iter().map(|(_, error)| error));
        return Err(Failure::from(last));
    }
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
        return Err(Failure::from(error));
    }
    builder
        .add_operation(kept, place.clone(), stage.as_deref())
        .map_err(Failure::from)
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
    parse_document_recovering(text, imports, kind).map_err(|errors| {
        errors
            .into_iter()
            .next()
            .expect("a document that fails has an error")
    })
}

/// As [`parse_document_with_imports`], giving every error that blanking
/// each line the first names, one after the other, would find: the first is
/// the error the other gives, and each of the rest comes only where blanking
/// the lines of those before it would not change what it is.
pub(crate) fn parse_document_recovering(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> Result<ParsedDocument, Vec<ParseError>> {
    let document = split_document(text);
    let lowered = lower(&parse_syntax(&document.pipeline), imports, kind);
    let inventory = match (kind, document.inventory_line) {
        (_, None) => None,
        (Kind::Recipe, Some(_)) => {
            Some(parse_source_inventory(&document.inventory).map_err(|error| vec![error])?)
        }
        // An error on an earlier line is the first.
        (Kind::Pipeline, Some(line)) => {
            let mut errors = lowered.as_ref().err().cloned().unwrap_or_default();
            let earlier = errors
                .iter()
                .take_while(|error| error.line() < line)
                .count();
            if earlier == errors.len() && !errors.is_empty() {
                return Err(errors);
            }
            errors.truncate(earlier);
            let records = document
                .inventory
                .lines()
                .enumerate()
                .filter(|(_, text)| !text.trim().is_empty())
                .map(|(index, _)| index + 1)
                .collect();
            errors.push(
                ParseError::new(
                    line,
                    "`sources:` and `contexts:` records belong in a .spitout, not a pipeline",
                )
                .with_kind(ParseErrorKind::MisplacedRecords { lines: records }),
            );
            return Err(errors);
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
