//! Reviews: which reviews judge which products, and the checks on a
//! pipeline's `review` declarations and the uses it makes of them.
//!
//! A review judges an artifact of a product it is attached to: a source
//! that names it, an operation output that names it or takes it from a
//! `review:` default, or an input port that names it. A person records the
//! verdicts, so a review never adds a job or a dependency; the resolver
//! withholds the artifacts a verdict fails, or that has none.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;

use crate::command::CommandError;
use crate::model::{
    stage_and_parents, DefaultReviews, Invocation, OperationDef, Pipeline, PipelineIndex, Port,
    ProductDef,
};
use crate::parser::SourceMap;
use crate::span::Place;

/// Check every `review` declaration and every use of one, collecting each
/// error. Uses on operations in `skip`, which already failed, are not
/// checked.
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
                    .at(lines.reviews.get(index).cloned())
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
    errors
}

/// The reviews that judge each product's artifacts as they are found or
/// made, found once for all the steps being resolved.
pub(crate) struct ReviewIndex<'p> {
    /// The reviews of each product that has any, by name, in the order they
    /// are attached.
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

    /// The reviews that judge `product`'s artifacts for every reader.
    pub(crate) fn of(&self, product: &str) -> &[&'p str] {
        self.products.get(product).map_or(&[], Vec::as_slice)
    }
}

/// `names` without a repeat, in order.
fn unique<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for name in names {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// The reviews of output `port` of `invocation`: the defaults of the file
/// and the stages around the step, less those the output opts out of, then
/// the reviews the output and the step attach.
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
        .filter(|(at, _)| *at == Port::Output(port))
        .map(|(_, review)| review.as_str());
    let attached = output.reviews.iter().map(String::as_str).chain(added);
    unique(reviews.iter().copied().chain(attached))
}

/// The reviews that judge what input `port` of the step `invocation` reads:
/// those its operation's port names, and those the step adds.
pub(crate) fn port_reviews<'p>(
    operation: &'p OperationDef,
    invocation: &'p Invocation,
    port: usize,
) -> Vec<&'p str> {
    let added = invocation
        .reviews
        .iter()
        .filter(|(at, _)| *at == Port::Input(port))
        .map(|(_, review)| review.as_str());
    unique(
        operation.inputs[port]
            .reviews
            .iter()
            .map(String::as_str)
            .chain(added),
    )
}

/// The products each review is attached to, by review: those it judges for
/// every reader and those a port reads, each once.
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
