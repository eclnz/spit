use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

mod checks;
mod entities;
mod expand;
pub(crate) mod imports;
mod reviews;
mod tools;

use checks::check_source_beside_paths;
use expand::BodyCheck;

use crate::model::{
    CallId, Cardinality, CheckDef, CheckId, CommandDef, CommandId, CoverageRule, Exclusion,
    InputBinding, InputRules, Invocation, OperationDef, Pipeline, Preference, ProductDef,
    ProductId, ResourcesDef, ResourcesId, SidecarGroup, SourceInventory, StageDef, StepOutput,
};
use crate::order::{order_dimensions, Output};
use crate::parser::error::LowerError;
use crate::parser::{
    document_records, empty_body, parse_syntax, without_bom, CommandPlaces, Kind, Layout,
    ParseError, ParseErrorKind, PathRule, Rule, SourceMap, Statement, StatementKind, Step, Syntax,
};
use crate::shape::{step_context, step_driver, BoundInput};
use crate::span::Place;
use crate::types::TypeExpr;
use imports::apply_import;

#[derive(Default)]
pub(crate) struct PipelineBuilder {
    pub(crate) pipeline: Pipeline,
    pub(crate) inputs: InputRules,
    pub(crate) lines: SourceMap,
    dimension_order: Option<(Vec<String>, Place)>,
    types_line: bool,
    entities: entities::EntitiesDeclarations,
    tools: tools::ToolLines,
    outputs: BTreeMap<String, Output>,
    product_ids: FxHashMap<String, ProductId>,
    sidecar_group_numbers: FxHashMap<String, usize>,
    operation_at: FxHashMap<String, usize>,
    intermediates: FxHashMap<String, CallId>,
    journal: Option<Vec<Undo>>,
}

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

    fn record(&mut self, undo: impl FnOnce() -> Undo) {
        if let Some(journal) = &mut self.journal {
            journal.push(undo());
        }
    }

    pub(crate) fn add_operation(
        &mut self,
        operation: OperationDef,
        place: Place,
        stage: Option<&str>,
    ) -> Result<(), ParseError> {
        if let Some(first) = self.lines.operations.get(&operation.name) {
            let name = &operation.name;
            let error = LowerError::DuplicateOperation {
                name: name.clone(),
                line: first.line,
                imported: self.lines.imported.contains(name),
                global: stage.is_some() || self.lines.operation_stages.contains_key(name),
            };
            return Err(ParseError::of(place.line, error).within(&place));
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

    pub(crate) fn add_command(&mut self, command: CommandDef, places: CommandPlaces) {
        let id = CommandId::at(self.pipeline.commands.len());
        self.lines.commands.push(id, places);
        self.pipeline.commands.push(command);
    }

    pub(crate) fn add_resources(&mut self, resources: ResourcesDef, place: Place) {
        let id = ResourcesId::at(self.pipeline.resources.len());
        self.lines.resources.push(id, place);
        self.pipeline.resources.push(resources);
    }

    pub(crate) fn add_check(&mut self, check: CheckDef, place: Place) {
        let id = CheckId::at(self.pipeline.checks.len());
        self.lines.checks.push(id, place);
        self.pipeline.checks.push(check);
    }

    fn add_invocation(&mut self, invocation: Invocation, step: &Step) {
        for output in &invocation.outputs {
            let before = self.lines.invocations.insert(output.clone(), step.clone());
            self.record(|| Undo::Invocation(output.clone(), before));
        }
        self.pipeline.invocations.push(invocation);
    }

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
            if pipeline.product_paths.contains_key(product) {
                return Err(ParseError::of(
                    line,
                    LowerError::DuplicatePathTemplateProduct {
                        product: product.to_string(),
                    },
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
                return Err(ParseError::of(
                    line,
                    LowerError::DuplicateDefaultPathTemplateStage {
                        stage: stage.to_string(),
                    },
                ));
            }
            definition.path_template = Some(template);
            lines.stage_paths.insert(stage.clone(), rule.place.clone());
        } else if pipeline.path_template.is_some() {
            return Err(ParseError::of(
                line,
                LowerError::DuplicateDefaultPathTemplate,
            ));
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
                Some(stage.to_owned()),
            ),
            None => (&mut self.pipeline.extension, None),
        };
        if slot.is_some() {
            return Err(ParseError::of(
                line,
                LowerError::DuplicateDefaultExt { stage: whose },
            ));
        }
        *slot = Some(extension.to_owned());
        Ok(())
    }

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
            let place = written
                .iter()
                .position(|name| *name == output.name)
                .map_or_else(|| step.call(), |index| step.output_at(index));
            self.add_product(product, place);
        }
        self.add_invocation(invocation, step);
    }
}

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
        let lowered = match (&statement.kind, kind) {
            (
                StatementKind::Constraint(..)
                | StatementKind::Exclude(..)
                | StatementKind::Prefer(..),
                Kind::Pipeline,
            ) => Err(ParseError::of(
                statement.place.line,
                LowerError::RulesBelongInRecipe,
            )),
            (StatementKind::ReviewFile { .. }, Kind::Pipeline) => Err(ParseError::of(
                statement.place.line,
                LowerError::ReviewFileLineInPipeline,
            )),
            (StatementKind::Review(review, _), Kind::Recipe) => Err(ParseError::of(
                statement.place.line,
                LowerError::ReviewFileWithoutReview {
                    name: review.name.to_string(),
                },
            )),
            _ => lower_statement(&mut builder, imports, statement, &mut errors),
        };
        if let Err(error) = lowered {
            errors.push(error.within(&statement.place));
        }
    }
    errors.extend(parsing.map(|(_, error)| error.clone()));
    let checks = [
        expand::check_bodies_have_no_commands(&builder),
        check_source_beside_paths(&builder),
        order_dimensions(
            &mut builder.pipeline.products,
            &builder.outputs,
            builder.dimension_order.as_ref(),
            &builder.lines,
        ),
        builder.entities.validate(&mut builder.pipeline),
        builder.tools.apply(&mut builder.pipeline, &builder.lines),
    ];
    errors.extend(checks.into_iter().filter_map(Result::err));
    (builder, errors)
}

fn lower_statement(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    statement: &Statement,
    errors: &mut Vec<ParseError>,
) -> Result<(), ParseError> {
    match &statement.kind {
        StatementKind::Import => apply_import(builder, imports, &statement.place)?,
        StatementKind::Root(folder) | StatementKind::OutputRoot(folder) => {
            let (slot, what) = match &statement.kind {
                StatementKind::Root(_) => (&mut builder.pipeline.root, "root"),
                _ => (&mut builder.pipeline.output, "output root"),
            };
            if slot.is_some() {
                return Err(ParseError::of(
                    statement.place.line,
                    LowerError::StatementRepeated {
                        what: what.to_string(),
                    },
                ));
            }
            *slot = Some((folder.clone(), statement.place.line));
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
                        ParseError::of(
                            place.line,
                            LowerError::BesideUnknownSource {
                                name: product.name.to_string(),
                                sibling: sibling.to_string(),
                            },
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
                    return Err(ParseError::of(
                        place.line,
                        LowerError::BesideWrongKind {
                            name: product.name.to_string(),
                            what: what.to_string(),
                            sibling: sibling.to_string(),
                        },
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
        StatementKind::Exclude(rule, reason, place) => {
            builder.inputs.exclusions.push(Exclusion {
                product: rule.product.clone(),
                values: rule.values.clone(),
                path: rule.path.clone(),
                reason: reason.clone(),
                origin: format!("line {}", statement.place.line),
            });
            builder.lines.exclusions.push(place.clone());
        }
        StatementKind::Prefer(path, reason) => {
            builder.inputs.preferences.push(Preference {
                path: path.clone(),
                reason: reason.clone(),
                origin: format!("line {}", statement.place.line),
            });
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
        StatementKind::Review(review, place) => {
            builder.add_review(review.clone(), place.clone());
        }
        StatementKind::ReviewFile {
            review,
            file,
            place,
        } => builder.add_review_file(review, file, place)?,
        StatementKind::DefaultReviews {
            stage,
            reviews,
            place,
        } => builder.add_default_reviews(stage.as_deref(), reviews, place)?,
        StatementKind::Dimensions(order) => {
            if builder.dimension_order.is_some() {
                return Err(ParseError::of(
                    statement.place.line,
                    LowerError::PipelineHasDimensionsLine,
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
                return Err(ParseError::of(
                    statement.place.line,
                    LowerError::TypeDeclaredWithOtherParameters {
                        name: name.to_string(),
                    },
                ));
            }
            builder
                .pipeline
                .type_decls
                .insert(name.clone(), params.clone());
        }
        StatementKind::Types(types) => {
            if builder.types_line {
                return Err(ParseError::of(
                    statement.place.line,
                    LowerError::PipelineHasTypesLine,
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
            extension,
            checks,
            place,
        } => builder.add_default_checks(stage.as_deref(), extension.as_deref(), checks, place)?,
        StatementKind::Extension { stage, extension } => {
            builder.add_extension(stage.as_deref(), extension, statement.place.line)?;
        }
        StatementKind::FlowStep(flow) => builder.add_flow_step(flow)?,
        kind @ (StatementKind::Tool(..)
        | StatementKind::Uses(..)
        | StatementKind::Anchored(..)
        | StatementKind::DefaultUses { .. }) => {
            let line = statement.place.line;
            builder.tools.add(&mut builder.pipeline, kind, line)?;
        }
    }
    Ok(())
}

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
    inputs
        .filter(|inputs| inputs.len() == invocation.inputs.len())
        .and_then(|inputs| {
            let (_, groups) = step_driver(&inputs)?;
            Some(step_context(&inputs, &groups))
        })
        .or_else(|| invocation.inputs.first().and_then(dimensions))
        .unwrap_or_default()
}

pub(crate) struct ParsedDocument {
    pub(crate) pipeline: Pipeline,
    pub(crate) inputs: InputRules,
    pub(crate) inventory: Option<SourceInventory>,
    pub(crate) lines: SourceMap,
}

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

pub(crate) fn parse_document_recovering(
    text: &str,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> (ParsedDocument, Vec<ParseError>) {
    parse_layout(&Layout::new(text), imports, kind)
}

pub(crate) fn parse_layout(
    layout: &Layout,
    imports: &BTreeMap<usize, Pipeline>,
    kind: Kind,
) -> (ParsedDocument, Vec<ParseError>) {
    let (builder, mut errors) = lower(&parse_syntax(layout), imports, kind);
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
            let at = errors
                .iter()
                .take_while(|error| error.line() < line)
                .count();
            errors.insert(
                at,
                ParseError::of(line, ParseErrorKind::MisplacedRecords { lines: records }),
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
