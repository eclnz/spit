//! Editor explanations of pipeline symbols, using the same compilation as
//! validation. No inventory is read and no dataset directories are scanned.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

mod operations;
mod signatures;

use operations::{call_details, composite_details, operation_details};
use signatures::{check_signature, operation_signature, product_signature};

use crate::builtins::{builtin_words, words_json};
use crate::compile::collect_pipeline;
use crate::diagnostics::{
    diagnostics_json, recover_document, shown_paths_json, Diagnostic, ShownPath,
};
use crate::imports::parse_located_document_recovering;
use crate::json::Json;
use crate::model::{Cardinality, PathOrigin, Pipeline, PipelineIndex, ProductDef};
use crate::parser::{without_bom, Kind};
use crate::paths::shown_path;
use crate::render::written_step;
use crate::span::{find_word, utf16_columns, Place};
use crate::types::TypeExpr;

/// An operation or product explanation at a precise source range. Lines and
/// UTF-16 columns are 1-based; the end column is exclusive, as in diagnostics.
/// Text is plain text, so editors can render it without trusting Markdown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hover {
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
    pub kind: HoverKind,
    pub name: String,
    pub signature: String,
    pub details: Vec<String>,
}

/// What a hover explains.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HoverKind {
    Product,
    Operation,
    Check,
}

impl HoverKind {
    /// The kind's name in `check --hovers` JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::Operation => "operation",
            Self::Check => "check",
        }
    }
}

/// Explain declarations and references in a pipeline, including unsaved
/// text and imported definitions. Broken lines are recovered; failed steps
/// retain declarations but never claim a successfully inferred result.
pub fn pipeline_hovers(text: &str, path: &Path) -> Vec<Hover> {
    let bom_column = usize::from(text.starts_with('\u{feff}'));
    let text = without_bom(text);
    let (document, _) = recover_document(text, |text| {
        parse_located_document_recovering(text, path, Kind::Pipeline)
    });
    let Some(document) = document else {
        return Vec::new();
    };
    let pipeline = &document.pipeline;
    let checked = collect_pipeline(pipeline);
    let steps = &checked.pipeline.steps;
    let steps_by_output: BTreeMap<_, _> = steps
        .iter()
        .map(|step| (step.invocation.output_product(), step))
        .collect();
    let inferred: BTreeMap<&str, &TypeExpr> = steps
        .iter()
        .flat_map(|step| {
            step.outputs
                .iter()
                .map(|(product, ty)| (product.name.as_str(), ty))
        })
        .collect();
    let products: BTreeMap<_, _> = pipeline
        .products
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|o| (o.name.as_str(), o))
        .collect();
    // Write shared product information and operation declarations once.
    // Calls add their own bindings and types without copying generic bodies.
    let index = PipelineIndex::new(pipeline);
    let consumers = consumers(pipeline);
    let product_infos: BTreeMap<&str, (String, Vec<String>)> = products
        .iter()
        .map(|(&name, product)| {
            let ty = inferred
                .get(name)
                .copied()
                .unwrap_or(&product.artifact_type);
            let used_by = consumers.get(name).map_or(&[][..], Vec::as_slice);
            let details = product_details(&index, product, inferred.get(name).copied(), used_by);
            (name, (product_signature(product, ty), details))
        })
        .collect();
    let operation_infos: BTreeMap<&str, (String, Vec<String>)> = operations
        .iter()
        .map(|(&name, operation)| {
            let info = (
                operation_signature(operation),
                operation_details(pipeline, operation),
            );
            (name, info)
        })
        .collect();
    let mut hovers = Vec::new();
    let text_lines: Vec<_> = text.lines().collect();
    let mut add =
        |place: Place, kind: HoverKind, name: &str, signature: String, details: Vec<String>| {
            let Some(line) = text_lines.get(place.line.saturating_sub(1)) else {
                return;
            };
            // Import declarations are located at the whole `use` line. Only
            // expose ranges that actually name this symbol in the current file.
            if line.get(place.columns.clone()) != Some(name) {
                return;
            }
            let columns = utf16_columns(line, &place.columns);
            hovers.push(Hover {
                line: place.line,
                column: columns.start + 1 + if place.line == 1 { bom_column } else { 0 },
                end_column: columns.end + 1 + if place.line == 1 { bom_column } else { 0 },
                kind,
                name: name.to_owned(),
                signature,
                details,
            });
        };
    let product_info = |name: &str| product_infos.get(name).cloned();
    let operation_info = |name: &str| operation_infos.get(name).cloned();
    for (name, place) in &document.lines.products {
        if let Some((signature, details)) = product_info(name) {
            add(place.clone(), HoverKind::Product, name, signature, details);
        }
    }
    for (check, place) in pipeline.checks.iter().zip(&document.lines.checks) {
        let details = vec![
            "Checks one artifact: a backend runs it on each artifact of a port or source that attaches it, and a nonzero exit fails the job.".to_owned(),
        ];
        add(
            place.clone(),
            HoverKind::Check,
            &check.name,
            check_signature(check),
            details,
        );
    }
    for (name, place) in &document.lines.operations {
        if let Some((signature, details)) = operation_info(name) {
            add(
                place.clone(),
                HoverKind::Operation,
                name,
                signature,
                details,
            );
        }
    }
    for invocation in &pipeline.invocations {
        // A step a call made is written as the call; the call's own hovers
        // follow.
        if invocation.origin.is_some() {
            continue;
        }
        let Some(location) = document.lines.invocations.get(invocation.output_product()) else {
            continue;
        };
        let compiled = steps_by_output.get(invocation.output_product()).copied();
        if let Some((signature, declaration)) = operation_infos.get(invocation.operation.as_str()) {
            let details = if let Some(step) = compiled {
                let mut details = call_details(step, &products, &inferred);
                details.extend(
                    declaration
                        .iter()
                        .filter(|detail| {
                            detail.starts_with("Command: ") || detail.starts_with("Verify: ")
                        })
                        .cloned(),
                );
                details
            } else {
                let mut details = declaration.clone();
                details.push(
                    "This call could not be checked; specialised types are unavailable.".to_owned(),
                );
                details
            };
            add(
                location.operation(),
                HoverKind::Operation,
                &invocation.operation,
                signature.clone(),
                details,
            );
        }
        for (index, name) in invocation.outputs.iter().enumerate() {
            if let Some((signature, details)) = product_info(name) {
                add(
                    location.output_at(index),
                    HoverKind::Product,
                    name,
                    signature,
                    details,
                );
            }
        }
        for (index, input) in invocation.inputs.iter().enumerate() {
            let name = input.product_name();
            if let (Some(mut place), Some((signature, mut details))) =
                (location.input(index), product_info(name))
            {
                // The diagnostic input range includes selectors; the hover
                // applies only to the product name, never a selector/value.
                place.columns.end = place.columns.start + name.len();
                if let Some(port) = compiled.and_then(|step| step.operation.inputs.get(index)) {
                    details.push(format!(
                        "Supplies port {} of {} ({} input).",
                        port.name,
                        invocation.operation,
                        cardinality(port.cardinality)
                    ));
                }
                add(place, HoverKind::Product, name, signature, details);
            }
        }
    }
    // The steps each written call expands to, grouped once.
    let mut expanded: Vec<Vec<String>> = vec![Vec::new(); pipeline.calls.len()];
    for step in &pipeline.invocations {
        if let Some(origin) = &step.origin {
            expanded[pipeline.written_call_id(origin.call).index()].push(written_step(step));
        }
    }
    for (call, steps) in pipeline.calls.iter().zip(&expanded) {
        if call.parent.is_some() {
            continue;
        }
        let Some(location) = call
            .outputs
            .first()
            .and_then(|output| document.lines.invocations.get(output))
        else {
            continue;
        };
        if let Some((signature, _)) = operation_infos.get(call.operation.as_str()) {
            let operation = operations[call.operation.as_str()];
            let mut details = composite_details(operation, call, &products, &inferred);
            details.push(format!("This call expands to: {}", steps.join("\n")));
            add(
                location.operation(),
                HoverKind::Operation,
                &call.operation,
                signature.clone(),
                details,
            );
        }
        let ports = operations
            .get(call.operation.as_str())
            .map(|operation| &operation.inputs);
        for (index, name) in call.inputs.iter().enumerate() {
            if let (Some(mut place), Some((signature, mut details))) =
                (location.input(index), product_info(name))
            {
                place.columns.end = place.columns.start + name.len();
                if let Some(port) = ports.and_then(|ports| ports.get(index)) {
                    details.push(format!(
                        "Supplies input {} of {}.",
                        port.name, call.operation
                    ));
                }
                add(place, HoverKind::Product, name, signature, details);
            }
        }
    }
    // Product names in path rules and operation names in commands are also
    // references. Search only before the template, not inside its literals.
    for (name, template) in &document.lines.paths {
        if let (Some(line), Some((signature, details))) = (
            text_lines.get(template.line.saturating_sub(1)),
            product_info(name),
        ) {
            if let Some(columns) = find_word(&line[..template.columns.start], 0, name) {
                add(
                    Place::new(template.line, columns),
                    HoverKind::Product,
                    name,
                    signature,
                    details,
                );
            }
        }
    }
    for (index, command) in pipeline.commands.iter().enumerate() {
        let Some(template) = document.lines.command(index) else {
            continue;
        };
        if let (Some(line), Some((signature, details))) = (
            text_lines.get(template.line.saturating_sub(1)),
            operation_info(&command.operation),
        ) {
            if let Some(columns) = find_word(&line[..template.columns.start], 0, &command.operation)
            {
                add(
                    Place::new(template.line, columns),
                    HoverKind::Operation,
                    &command.operation,
                    signature,
                    details,
                );
            }
        }
    }
    hovers.sort_by_key(|hover| (hover.line, hover.column, hover.end_column));
    hovers
        .dedup_by(|a, b| a.line == b.line && a.column == b.column && a.end_column == b.end_column);
    hovers
}

fn cardinality(value: Cardinality) -> &'static str {
    match value {
        Cardinality::One => "one",
        Cardinality::Many => "many",
    }
}

/// Each product's readers, as `averaged = average(…)`, in step order. A
/// product a call reads is read by the call, as it is written.
fn consumers(pipeline: &Pipeline) -> BTreeMap<&str, Vec<String>> {
    let mut consumers: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for step in &pipeline.invocations {
        let written = step
            .origin
            .as_ref()
            .map(|origin| pipeline.written_call(origin.call))
            .filter(|call| {
                step.inputs
                    .iter()
                    .any(|input| call.inputs.contains(&input.product))
            });
        let read: BTreeSet<&str> = step
            .inputs
            .iter()
            .map(|input| input.product_name())
            .collect();
        for product in read {
            let reader = match written {
                Some(call) if call.inputs.iter().any(|input| input == product) => {
                    format!("{} = {}(…)", call.outputs.join(", "), call.operation)
                }
                _ => format!("{} = {}(…)", step.outputs.join(", "), step.operation),
            };
            let readers = consumers.entry(product).or_default();
            // A call's steps that read one product are one reader.
            if readers.last() != Some(&reader) {
                readers.push(reader);
            }
        }
    }
    consumers
}

/// A step as written, as `cleaned = clean(reads, table)`.
fn product_details(
    index: &PipelineIndex<'_>,
    product: &ProductDef,
    inferred: Option<&TypeExpr>,
    consumers: &[String],
) -> Vec<String> {
    let producer = index.producer(&product.name).map(|(call, _)| call);
    let written = producer
        .and_then(|step| step.origin.as_ref())
        .map(|origin| index.pipeline.written_call(origin.call));
    let mut details = vec![match (producer, written) {
        (Some(step), Some(call)) => format!(
            "Derived product. Produced by {}({}), by its step {}.",
            call.operation,
            call.inputs.join(", "),
            written_step(step)
        ),
        (Some(step), None) => format!(
            "Derived product. Produced by {}({}).",
            step.operation,
            step.inputs
                .iter()
                .map(|input| input.product_name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        (None, _) => "Source product: a family of input artifacts.".to_owned(),
    }];
    if producer.is_some() && inferred.is_none() {
        details.push(
            "Inference unavailable because this step or a dependency could not be checked."
                .to_owned(),
        );
    }
    if producer.is_some()
        && product.artifact_type != TypeExpr::Unknown
        && inferred.is_some_and(|ty| *ty != product.artifact_type)
    {
        details.push(format!(
            "Declared type: {}. {}",
            product.artifact_type, "Signature shows the compiler-inferred type."
        ));
    }
    if !consumers.is_empty() {
        const SHOWN_CONSUMERS: usize = 8;
        details.push(format!(
            "Used by: {}",
            consumers[..consumers.len().min(SHOWN_CONSUMERS)].join("; ")
        ));
        if consumers.len() > SHOWN_CONSUMERS {
            details.push(format!(
                "{} other steps also use this product.",
                consumers.len() - SHOWN_CONSUMERS
            ));
        }
    }
    if let Some(stage) = index.stage_of(&product.name) {
        details.push(format!("Stage: {stage}"));
    }
    let resolved_path = || {
        index
            .path_template_for(&product.name)
            .map(|template| shown_path(index, &product.name, &template))
    };
    let path = resolved_path().unwrap_or_default();
    details.push(
        match (
            index.beside(&product.name),
            index.path_origin(&product.name),
        ) {
            (Some((sibling, _, _)), _) => format!("Path template: {path} (beside {sibling})."),
            (None, Some((PathOrigin::Explicit, _))) => {
                format!("Path template: {path} (explicit product rule).")
            }
            (None, Some((PathOrigin::Stage(stage), _))) => {
                format!("Path template: {path} (inherited from stage {stage}).")
            }
            (None, Some((PathOrigin::BuiltIn, _))) => {
                format!("Path template: {path} (built-in output default).")
            }
            (None, Some((PathOrigin::Default, _))) => {
                format!("Path template: {path} (pipeline default).")
            }
            (None, None) => {
                "No pipeline path rule; a recipe or inventory must supply the source path."
                    .to_owned()
            }
        },
    );
    details
}

/// Hovers and resolved path hints added to `check --json --hovers` for a
/// pipeline, with the built-in words it uses. All strings are escaped by
/// the shared JSON writer, and ranges use the diagnostics convention.
pub fn render_editor_json(
    diagnostics: &[Diagnostic],
    text: &str,
    path: &Path,
    paths: &[ShownPath],
) -> String {
    let hovers = pipeline_hovers(text, path);
    let words = builtin_words(text);
    let [words, word_docs] = words_json(&words);
    format!(
        "{}\n",
        Json::object([
            ("diagnostics", diagnostics_json(diagnostics, text, None)),
            ("paths", shown_paths_json(paths)),
            ("hovers", hovers_json(&hovers)),
            words,
            word_docs,
        ])
    )
}

/// `check --json --hovers` for a recipe or a `.spitout`: its diagnostics and
/// the built-in words it uses. Their names come from a pipeline, which
/// explains them. Diagnostics about records point into `records_text`.
pub fn render_words_json(
    diagnostics: &[Diagnostic],
    text: &str,
    records_text: Option<&str>,
) -> String {
    let words = builtin_words(text);
    let [words, word_docs] = words_json(&words);
    format!(
        "{}\n",
        Json::object([
            (
                "diagnostics",
                diagnostics_json(diagnostics, text, records_text)
            ),
            words,
            word_docs,
        ])
    )
}

fn hovers_json(hovers: &[Hover]) -> Json<'_> {
    Json::array(hovers.iter().map(|hover| {
        Json::object([
            ("line", Json::Number(hover.line)),
            ("column", Json::Number(hover.column)),
            ("end_column", Json::Number(hover.end_column)),
            ("kind", Json::string(hover.kind.name())),
            ("name", Json::string(&hover.name)),
            ("signature", Json::string(&hover.signature)),
            (
                "details",
                Json::array(hover.details.iter().map(Json::string)),
            ),
        ])
    }))
}
