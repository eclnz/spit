//! One dimension order for a pipeline. Each source lists its dimensions in
//! that order, and a `dimensions [...]` line declares it where the sources
//! leave a pair unordered. Every product a step makes takes its dimensions
//! in the same order, so collections, `{entities}` and displayed
//! identities agree across the pipeline.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::ProductDef;
use crate::parser::{ParseError, SourceMap};
use crate::span::Place;

/// What a step's output declared about its dimensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Output {
    /// Inferred from the step; the order sorts them.
    Inferred,
    /// Written in the step, as in `summary : Summary [model, config] = ...`;
    /// the order checks them.
    Annotated,
}

/// The order between a pipeline's dimensions: a total order from a
/// `dimensions` line, or the partial order the sources give.
struct Order {
    /// Every dimension, by first appearance.
    dimensions: Vec<String>,
    /// `before[a]` holds every dimension `a` comes before.
    before: Vec<BTreeSet<usize>>,
}

impl Order {
    fn index(&self, dimension: &str) -> usize {
        self.dimensions
            .iter()
            .position(|known| known == dimension)
            .expect("every dimension is known")
    }

    fn precedes(&self, first: &str, second: &str) -> bool {
        self.before[self.index(first)].contains(&self.index(second))
    }

    /// Record that `first` comes before `second`, and all that follows.
    fn add(&mut self, first: usize, second: usize) {
        let mut later: BTreeSet<usize> = self.before[second].clone();
        later.insert(second);
        for index in 0..self.dimensions.len() {
            if index == first || self.before[index].contains(&first) {
                self.before[index].extend(later.iter().copied());
            }
        }
    }

    /// `dimensions` in this order, or the first pair nothing orders.
    fn sort(&self, dimensions: &[String]) -> Result<Vec<String>, (String, String)> {
        for (position, first) in dimensions.iter().enumerate() {
            for second in &dimensions[position + 1..] {
                if !self.precedes(first, second) && !self.precedes(second, first) {
                    return Err((first.clone(), second.clone()));
                }
            }
        }
        let mut sorted = dimensions.to_vec();
        sorted.sort_by_key(|dimension| {
            dimensions
                .iter()
                .filter(|other| self.precedes(other, dimension))
                .count()
        });
        Ok(sorted)
    }

    /// Every dimension in an order that agrees with this one, taking the
    /// first to appear whenever several could come next.
    fn suggestion(&self) -> Vec<String> {
        let mut placed: Vec<usize> = Vec::new();
        while placed.len() < self.dimensions.len() {
            let next = (0..self.dimensions.len())
                .find(|candidate| {
                    !placed.contains(candidate)
                        && (0..self.dimensions.len()).all(|other| {
                            placed.contains(&other) || !self.before[other].contains(candidate)
                        })
                })
                .expect("the order has no cycle");
            placed.push(next);
        }
        placed
            .into_iter()
            .map(|index| self.dimensions[index].clone())
            .collect()
    }
}

/// Order every product's dimensions. `outputs` names each product a step
/// makes; every other product is a source. `declared` is the `dimensions`
/// line, if the pipeline has one.
pub(crate) fn order_dimensions(
    products: &mut [ProductDef],
    outputs: &BTreeMap<String, Output>,
    declared: Option<&(Vec<String>, Place)>,
    lines: &SourceMap,
) -> Result<(), ParseError> {
    let mut dimensions: Vec<String> = Vec::new();
    for product in products.iter() {
        for dimension in &product.dimensions {
            if !dimensions.contains(dimension) {
                dimensions.push(dimension.clone());
            }
        }
    }
    let count = dimensions.len();
    let mut order = Order {
        dimensions,
        before: vec![BTreeSet::new(); count],
    };
    let place = |name: &str| lines.products.get(name).cloned().unwrap_or_default();
    let sources = || {
        products
            .iter()
            .filter(|product| !outputs.contains_key(&product.name))
    };
    match declared {
        Some((declared, line)) => {
            declare(&mut order, declared, line, products)?;
            for source in sources() {
                if order.sort(&source.dimensions).ok().as_ref() != Some(&source.dimensions) {
                    let error = ParseError::new(
                        place(&source.name).line,
                        format!(
                            "`{}` lists its dimensions as [{}], but `dimensions` orders them [{}]",
                            source.name,
                            source.dimensions.join(", "),
                            in_order(declared, &source.dimensions).join(", "),
                        ),
                    );
                    return Err(error.within(&place(&source.name)));
                }
            }
        }
        None => {
            let mut seen: Vec<&ProductDef> = Vec::new();
            for source in sources() {
                for (position, first) in source.dimensions.iter().enumerate() {
                    for second in &source.dimensions[position + 1..] {
                        if order.precedes(second, first) {
                            return Err(conflict(source, first, second, &seen, &place));
                        }
                        let (first, second) = (order.index(first), order.index(second));
                        order.add(first, second);
                    }
                }
                seen.push(source);
            }
        }
    }
    for product in products.iter_mut() {
        let Some(output) = outputs.get(&product.name) else {
            continue;
        };
        let at = place(&product.name);
        let sorted = order.sort(&product.dimensions).map_err(|(first, second)| {
            ParseError::new(
                at.line,
                format!(
                    "`{}` has dimensions `{first}` and `{second}`, which no source orders; \
                     declare the order once with `dimensions [{}]`",
                    product.name,
                    order.suggestion().join(", "),
                ),
            )
            .within(&at)
        })?;
        match output {
            Output::Inferred => product.dimensions = sorted,
            Output::Annotated if sorted != product.dimensions => {
                return Err(ParseError::new(
                    at.line,
                    format!(
                        "`{}` lists its dimensions as [{}], but the pipeline orders them [{}]",
                        product.name,
                        product.dimensions.join(", "),
                        sorted.join(", "),
                    ),
                )
                .within(&at));
            }
            Output::Annotated => {}
        }
    }
    Ok(())
}

/// Take a `dimensions` line as the whole order: it must name every
/// dimension the products have, and no other.
fn declare(
    order: &mut Order,
    declared: &[String],
    line: &Place,
    products: &[ProductDef],
) -> Result<(), ParseError> {
    let fail = |message: String| Err(ParseError::new(line.line, message).within(line));
    if let Some(unknown) = declared
        .iter()
        .find(|dimension| !order.dimensions.contains(dimension))
    {
        return fail(format!(
            "`dimensions` names `{unknown}`, which no product has"
        ));
    }
    if let Some(missing) = order
        .dimensions
        .iter()
        .find(|dimension| !declared.contains(dimension))
    {
        let product = products
            .iter()
            .find(|product| product.dimensions.contains(missing))
            .map_or_else(String::new, |product| {
                format!(", a dimension of `{}`", product.name)
            });
        return fail(format!("`dimensions` leaves out `{missing}`{product}"));
    }
    for (position, first) in declared.iter().enumerate() {
        for second in &declared[position + 1..] {
            let (first, second) = (order.index(first), order.index(second));
            order.add(first, second);
        }
    }
    Ok(())
}

/// The error for `source`, which lists `first` before `second` when the
/// sources before it put `second` first.
fn conflict(
    source: &ProductDef,
    first: &str,
    second: &str,
    seen: &[&ProductDef],
    place: &impl Fn(&str) -> Place,
) -> ParseError {
    let earlier = seen.iter().find(|other| {
        let position = |dimension: &str| other.dimensions.iter().position(|d| d == dimension);
        matches!((position(second), position(first)), (Some(s), Some(f)) if s < f)
    });
    let by = earlier.map_or_else(
        || "other sources put".to_owned(),
        |other| format!("`{}` puts", other.name),
    );
    let at = place(&source.name);
    ParseError::new(
        at.line,
        format!(
            "`{}` lists `{first}` before `{second}`, but {by} `{second}` first; \
             list them in one order, or declare it with `dimensions [...]`",
            source.name
        ),
    )
    .within(&at)
}

/// `dimensions` in the order `declared` gives them.
fn in_order(declared: &[String], dimensions: &[String]) -> Vec<String> {
    declared
        .iter()
        .filter(|dimension| dimensions.contains(dimension))
        .cloned()
        .collect()
}
