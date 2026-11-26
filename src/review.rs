mod evidence;

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;

use crate::model::{
    stage_and_parents, DefaultReviews, Invocation, OperationDef, Pipeline, PipelineIndex, Port,
    ProductDef, ReviewId,
};
use crate::parser::SourceMap;
use crate::span::Place;
use crate::template::CommandError;

pub(crate) fn collect_reviews(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<CommandError> {
    let mut errors = Vec::new();
    let mut declared = BTreeSet::new();
    for (index, review) in pipeline.reviews.iter().enumerate() {
        if !declared.insert(review.name.as_str()) {
            errors.push(
                CommandError::new(format!("duplicate review `{}`", review.name))
                    .at(lines.reviews.get(ReviewId::at(index)).cloned())
                    .focus(&review.name),
            );
        }
    }
    let mut uses = |names: &[String], place: Option<Place>| {
        for name in names {
            if !declared.contains(name.as_str()) {
                errors.push(
                    CommandError::new(format!(
                        "unknown review `{name}`; declare it with `review {name}`"
                    ))
                    .at(place.clone())
                    .focus(name),
                );
            }
        }
    };
    for operation in &pipeline.operations {
        if skip.contains(&operation.name) {
            continue;
        }
        let place = lines.operations.get(&operation.name).cloned();
        for port in &operation.inputs {
            uses(&port.reviews, place.clone());
        }
        for port in &operation.outputs {
            uses(&port.reviews, place.clone());
            uses(&port.exempt_reviews, place.clone());
        }
    }
    let mut has_evidence = false;
    for invocation in &pipeline.invocations {
        if invocation.reviews.is_empty() {
            continue;
        }
        let place = lines
            .invocations
            .get(invocation.output_product())
            .map(|step| step.call());
        for used in &invocation.reviews {
            has_evidence |= used.evidence.is_some();
            uses(std::slice::from_ref(&used.name), place.clone());
        }
    }
    for (defaults, place) in
        std::iter::once((&pipeline.default_reviews, lines.default_reviews.clone())).chain(
            pipeline.stages.iter().map(|stage| {
                (
                    &stage.reviews,
                    lines.stage_reviews.get(&stage.name).cloned(),
                )
            }),
        )
    {
        uses(&defaults.reviews, place.clone());
        uses(&defaults.exempt, place);
    }
    for product in &pipeline.products {
        uses(&product.reviews, lines.products.get(&product.name).cloned());
    }
    if has_evidence {
        let index = PipelineIndex::new(pipeline);
        errors.extend(evidence_errors(&index, lines, skip));
        if errors.is_empty() && skip.is_empty() {
            errors.extend(evidence::dependency_error(&index, lines));
        }
    }
    errors
}

fn evidence_errors(
    index: &PipelineIndex<'_>,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<CommandError> {
    let mut errors = Vec::new();
    for invocation in &index.pipeline.invocations {
        let mut pairs = BTreeSet::new();
        for used in &invocation.reviews {
            let (Port::Output(port), Some(evidence)) = (used.port, &used.evidence) else {
                continue;
            };
            let Some(candidate) = invocation
                .outputs
                .get(port)
                .and_then(|name| index.product(name))
            else {
                continue;
            };
            if skip.contains(&candidate.name) || skip.contains(evidence) {
                continue;
            }
            let detail = if !pairs.insert((port, &used.name)) {
                format!(
                    "review `{}` of `{}` declares evidence more than once",
                    used.name, candidate.name
                )
            } else if let Some(evidence_product) = index.product(evidence) {
                let Some(dimension) = evidence_product
                    .dimensions
                    .iter()
                    .find(|name| !candidate.dimensions.contains(name))
                else {
                    continue;
                };
                format!("review `{}` of `{}` cannot match evidence `{evidence}` by `{dimension}`; that dimension is missing from `{}`",
                    used.name, candidate.name, candidate.name)
            } else {
                format!(
                    "unknown evidence product `{evidence}` for review `{}` of `{}`",
                    used.name, candidate.name
                )
            };
            let place = invocation
                .origin
                .as_ref()
                .map(|origin| origin.step.clone())
                .or_else(|| {
                    lines
                        .invocations
                        .get(invocation.output_product())
                        .map(|step| step.call())
                });
            errors.push(CommandError::new(detail).at(place).focus(evidence));
        }
    }
    errors
}

#[derive(Default)]
pub(crate) struct ReviewIndex<'p> {
    products: FxHashMap<&'p str, Vec<&'p str>>,
}

impl<'p> ReviewIndex<'p> {
    pub(crate) fn new(index: &PipelineIndex<'p>) -> Self {
        let pipeline = index.pipeline;
        let mut products: FxHashMap<&'p str, Vec<&'p str>> = FxHashMap::default();
        let nothing_declared = pipeline.reviews.is_empty();
        let stages: FxHashMap<&str, &DefaultReviews> = pipeline
            .stages
            .iter()
            .map(|stage| (stage.name.as_str(), &stage.reviews))
            .collect();
        for product in &pipeline.products {
            if products.contains_key(product.name.as_str()) || nothing_declared {
                continue;
            }
            let reviews = match index.producer(&product.name) {
                None => unique(product.reviews.iter().map(String::as_str)),
                Some((invocation, port)) => index
                    .operation(&invocation.operation)
                    .map(|operation| output_reviews(pipeline, &stages, operation, invocation, port))
                    .unwrap_or_default(),
            };
            if !reviews.is_empty() {
                products.insert(product.name.as_str(), reviews);
            }
        }
        Self { products }
    }

    pub(crate) fn of(&self, product: &str) -> &[&'p str] {
        self.products.get(product).map_or(&[], Vec::as_slice)
    }
}

fn unique<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for name in names {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

fn output_reviews<'p>(
    pipeline: &'p Pipeline,
    stages: &FxHashMap<&str, &'p DefaultReviews>,
    operation: &'p OperationDef,
    invocation: &'p Invocation,
    port: usize,
) -> Vec<&'p str> {
    let output = &operation.outputs[port];
    let mut reviews: Vec<&str> = Vec::new();
    let mut apply = |defaults: &'p DefaultReviews| {
        for name in &defaults.reviews {
            if !reviews.contains(&name.as_str()) {
                reviews.push(name);
            }
        }
        reviews.retain(|name| !defaults.exempt.iter().any(|exempt| exempt == name));
    };
    apply(&pipeline.default_reviews);
    if let Some(stage) = invocation.stage.as_deref() {
        let mut around: Vec<&str> = stage_and_parents(stage).collect();
        around.reverse();
        for name in around {
            if let Some(defaults) = stages.get(name) {
                apply(defaults);
            }
        }
    }
    reviews.retain(|name| !output.exempt_reviews.iter().any(|exempt| exempt == name));
    let added = invocation
        .reviews
        .iter()
        .filter(|used| used.port == Port::Output(port))
        .map(|used| used.name.as_str());
    let attached = output.reviews.iter().map(String::as_str).chain(added);
    unique(reviews.iter().copied().chain(attached))
}

pub(crate) fn port_reviews<'p>(
    operation: &'p OperationDef,
    invocation: &'p Invocation,
    port: usize,
) -> Vec<&'p str> {
    let added = invocation
        .reviews
        .iter()
        .filter(|used| used.port == Port::Input(port))
        .map(|used| used.name.as_str());
    unique(
        operation.inputs[port]
            .reviews
            .iter()
            .map(String::as_str)
            .chain(added),
    )
}

pub(crate) fn attached_products<'p>(
    index: &PipelineIndex<'p>,
    reviews: &ReviewIndex<'p>,
) -> BTreeMap<&'p str, Vec<&'p ProductDef>> {
    let mut attached: BTreeMap<&str, Vec<&ProductDef>> = BTreeMap::new();
    let mut attach = |review: &'p str, product: &'p ProductDef| {
        let products = attached.entry(review).or_default();
        if !products.iter().any(|known| known.name == product.name) {
            products.push(product);
        }
    };
    for (&name, names) in &reviews.products {
        if let Some(product) = index.product(name) {
            names.iter().for_each(|&review| attach(review, product));
        }
    }
    for invocation in &index.pipeline.invocations {
        let Some(operation) = index.operation(&invocation.operation) else {
            continue;
        };
        for (port, binding) in invocation
            .inputs
            .iter()
            .enumerate()
            .take(operation.inputs.len())
        {
            let Some(product) = index.product(binding.product_name()) else {
                continue;
            };
            for review in port_reviews(operation, invocation, port) {
                attach(review, product);
            }
        }
    }
    attached
}
