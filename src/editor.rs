//! Editor explanations of pipeline symbols, using the same compilation as
//! validation. No inventory is read and no dataset directories are scanned.

use std::collections::BTreeMap;
use std::path::Path;

use crate::compile::{collect_pipeline, CompiledStep};
use crate::diagnostics::{diagnostics_json, recover_document, Diagnostic};
use crate::imports::parse_located_document;
use crate::json::Json;
use crate::model::{Cardinality, CommandRole, OperationDef, Pipeline, ProductDef};
use crate::parser::{without_bom, Kind};
use crate::paths::PathTemplate;
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
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub details: Vec<String>,
}

/// Explain declarations and references in a pipeline, including unsaved
/// text and imported definitions. Broken lines are recovered; failed steps
/// retain declarations but never claim a successfully inferred result.
pub fn pipeline_hovers(text: &str, path: &Path) -> Vec<Hover> {
    let bom_column = usize::from(text.starts_with('\u{feff}'));
    let text = without_bom(text);
    let (document, _) = recover_document(text, |text| {
        parse_located_document(text, path, Kind::Pipeline)
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
    let mut hovers = Vec::new();
    let text_lines: Vec<_> = text.lines().collect();
    let mut add =
        |place: Place, kind: &str, name: &str, signature: String, details: Vec<String>| {
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
                kind: kind.to_owned(),
                name: name.to_owned(),
                signature,
                details,
            });
        };
    let product_info = |name: &str| {
        products.get(name).map(|product| {
            let ty = inferred
                .get(name)
                .copied()
                .unwrap_or(&product.artifact_type);
            (
                product_signature(product, ty),
                product_details(pipeline, product, inferred.contains_key(name)),
            )
        })
    };
    for (name, place) in &document.lines.products {
        if let Some((signature, details)) = product_info(name) {
            add(place.clone(), "product", name, signature, details);
        }
    }
    for (name, place) in &document.lines.operations {
        if let Some(operation) = operations.get(name.as_str()) {
            add(
                place.clone(),
                "operation",
                name,
                operation_signature(operation),
                operation_details(pipeline, operation),
            );
        }
    }
    for invocation in &pipeline.invocations {
        let Some(location) = document.lines.invocations.get(invocation.output_product()) else {
            continue;
        };
        let compiled = steps_by_output.get(invocation.output_product()).copied();
        if let Some(operation) = operations.get(invocation.operation.as_str()) {
            let mut details = operation_details(pipeline, operation);
            if let Some(step) = compiled {
                details.extend(call_details(step, &products, &inferred));
            } else {
                details.push(
                    "This call could not be checked; specialised types are unavailable.".to_owned(),
                );
            }
            add(
                location.operation(),
                "operation",
                &operation.name,
                operation_signature(operation),
                details,
            );
        }
        for (index, name) in invocation.outputs.iter().enumerate() {
            if let Some((signature, details)) = product_info(name) {
                add(
                    location.output_at(index),
                    "product",
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
                add(place, "product", name, signature, details);
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
                    "product",
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
        if let (Some(line), Some(operation)) = (
            text_lines.get(template.line.saturating_sub(1)),
            operations.get(command.operation.as_str()),
        ) {
            if let Some(columns) = find_word(&line[..template.columns.start], 0, &command.operation)
            {
                add(
                    Place::new(template.line, columns),
                    "operation",
                    &operation.name,
                    operation_signature(operation),
                    operation_details(pipeline, operation),
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

fn product_signature(product: &ProductDef, ty: &TypeExpr) -> String {
    format!(
        "{}: {} [{}]",
        product.name,
        ty,
        product.dimensions.join(", ")
    )
}

fn operation_signature(operation: &OperationDef) -> String {
    let inputs = operation
        .inputs
        .iter()
        .map(|port| {
            format!(
                "{}: {}{}",
                port.name,
                if port.cardinality == Cardinality::Many {
                    "many "
                } else {
                    ""
                },
                port.artifact_type
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let outputs = if operation.outputs.len() == 1 && operation.outputs[0].name == "output" {
        operation.outputs[0].artifact_type.to_string()
    } else {
        format!(
            "({})",
            operation
                .outputs
                .iter()
                .map(|port| format!("{}: {}", port.name, port.artifact_type))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut signature = format!("operation {}({inputs}) -> {outputs}", operation.name);
    if let Some(dimension) = &operation.aggregated_dimension {
        signature.push_str(&format!(" @ drop({dimension})"));
    }
    if let Some(minimum) = operation.minimum_collection {
        signature.push_str(&format!(" @ min({minimum})"));
    }
    signature
}

fn operation_details(pipeline: &Pipeline, operation: &OperationDef) -> Vec<String> {
    let mut details = Vec::new();
    if operation
        .inputs
        .iter()
        .any(|port| port.cardinality == Cardinality::Many)
    {
        details.push("Groups a many input into one job per remaining context.".to_owned());
    } else {
        details.push(
            "Single-artifact inputs are matched for each job; outputs preserve the job's dimensions.".to_owned(),
        );
    }
    for command in pipeline
        .commands
        .iter()
        .filter(|command| command.operation == operation.name)
    {
        details.push(format!(
            "{}: {}",
            match command.role {
                CommandRole::Run => "Command",
                CommandRole::Verify => "Verify",
            },
            command.template
        ));
    }
    details
}

fn call_details(
    step: &CompiledStep<'_>,
    products: &BTreeMap<&str, &ProductDef>,
    inferred: &BTreeMap<&str, &TypeExpr>,
) -> Vec<String> {
    let mut details = vec!["This call:".to_owned()];
    for (port, binding) in step.operation.inputs.iter().zip(&step.invocation.inputs) {
        if let Some(product) = products.get(binding.product_name()) {
            let ty = inferred
                .get(product.name.as_str())
                .copied()
                .unwrap_or(&product.artifact_type);
            details.push(format!(
                "{} ← {} ({} input; expects {})",
                port.name,
                product_signature(product, ty),
                cardinality(port.cardinality),
                step.substitutions
                    .substitute(&port.artifact_type)
                    .erase_variables()
            ));
        }
    }
    for (port, (product, ty)) in step.operation.outputs.iter().zip(&step.outputs) {
        details.push(format!(
            "{} → {}",
            port.name,
            product_signature(product, ty)
        ));
    }
    if !step.substitutions.0.is_empty() {
        details.push(format!(
            "Type bindings: {}",
            step.substitutions
                .0
                .iter()
                .map(|(name, ty)| format!("{name} = {}", step.substitutions.substitute(ty)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    details
}

fn product_details(pipeline: &Pipeline, product: &ProductDef, inferred: bool) -> Vec<String> {
    let producer = pipeline
        .invocations
        .iter()
        .find(|call| call.outputs.contains(&product.name));
    let mut details = vec![match producer {
        Some(call) => format!(
            "Derived product. Produced by {}({}).",
            call.operation,
            call.inputs
                .iter()
                .map(|input| input.product_name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => "Source product: a family of input artifacts.".to_owned(),
    }];
    if producer.is_some() {
        details.push(format!(
            "Declared type: {}. {}",
            product.artifact_type,
            if inferred {
                "Signature shows the compiler-inferred type."
            } else {
                "Inference unavailable because this step or a dependency could not be checked."
            }
        ));
    }
    let consumers = pipeline
        .invocations
        .iter()
        .filter(|call| {
            call.inputs
                .iter()
                .any(|input| input.product_name() == product.name)
        })
        .map(|call| format!("{} = {}(…)", call.outputs.join(", "), call.operation))
        .collect::<Vec<_>>();
    if !consumers.is_empty() {
        details.push(format!("Used by: {}", consumers.join("; ")));
    }
    if let Some(stage) = pipeline.stage_of(&product.name) {
        details.push(format!("Stage: {stage}"));
    }
    if let Some(template) = pipeline.product_paths.get(&product.name) {
        details.push(format!(
            "Path template: {template} (explicit product rule)."
        ));
    } else if let Some((stage, template)) = pipeline.stage_path_rule(&product.name) {
        details.push(format!(
            "Path template: {template} (inherited from stage {stage})."
        ));
    } else if let Some(template) = &pipeline.path_template {
        details.push(format!("Path template: {template} (pipeline default)."));
    } else if producer.is_some() {
        details.push(format!(
            "Path template: {} (built-in output default).",
            PathTemplate::default_output()
        ));
    } else {
        details.push(
            "No pipeline path rule; a recipe or inventory must supply the source path.".to_owned(),
        );
    }
    details
}

/// The hover array added to `check --json --hovers`. All strings are escaped
/// by the shared JSON writer, and ranges use the diagnostics convention.
pub fn render_editor_json(diagnostics: &[Diagnostic], text: &str, path: &Path) -> String {
    let hovers = pipeline_hovers(text, path);
    format!(
        "{}\n",
        Json::object([
            ("diagnostics", diagnostics_json(diagnostics, text, None)),
            ("hovers", hovers_json(&hovers)),
        ])
    )
}

fn hovers_json(hovers: &[Hover]) -> Json<'_> {
    Json::array(hovers.iter().map(|hover| {
        Json::object([
            ("line", Json::Number(hover.line)),
            ("column", Json::Number(hover.column)),
            ("end_column", Json::Number(hover.end_column)),
            ("kind", Json::string(&hover.kind)),
            ("name", Json::string(&hover.name)),
            ("signature", Json::string(&hover.signature)),
            (
                "details",
                Json::array(hover.details.iter().map(Json::string)),
            ),
        ])
    }))
}
