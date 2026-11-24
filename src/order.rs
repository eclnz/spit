use std::collections::{BTreeMap, BTreeSet};

use crate::model::ProductDef;
use crate::parser::error::OrderError;
use crate::parser::{ParseError, SourceMap};
use crate::span::Place;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Output {
    Inferred,
    Annotated,
}

struct Order {
    dimensions: Vec<String>,
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

    fn add(&mut self, first: usize, second: usize) {
        let mut later: BTreeSet<usize> = self.before[second].clone();
        later.insert(second);
        for index in 0..self.dimensions.len() {
            if index == first || self.before[index].contains(&first) {
                self.before[index].extend(later.iter().copied());
            }
        }
    }

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
                    let error = ParseError::of(
                        place(&source.name).line,
                        OrderError::SourceOrderDisagrees {
                            name: source.name.to_string(),
                            dimensions: source.dimensions.join(", "),
                            ordered: (in_order(declared, &source.dimensions).join(", "))
                                .to_string(),
                        },
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
            ParseError::of(
                at.line,
                OrderError::DimensionsUnordered {
                    name: product.name.to_string(),
                    first: first.to_string(),
                    second: second.to_string(),
                    suggestion: (order.suggestion().join(", ")).to_string(),
                },
            )
            .within(&at)
        })?;
        match output {
            Output::Inferred => product.dimensions = sorted,
            Output::Annotated if sorted != product.dimensions => {
                return Err(ParseError::of(
                    at.line,
                    OrderError::ProductOrderDisagrees {
                        name: product.name.to_string(),
                        dimensions: product.dimensions.join(", "),
                        sorted: sorted.join(", "),
                    },
                )
                .within(&at));
            }
            Output::Annotated => {}
        }
    }
    Ok(())
}

fn declare(
    order: &mut Order,
    declared: &[String],
    line: &Place,
    products: &[ProductDef],
) -> Result<(), ParseError> {
    let fail = |error: OrderError| Err(ParseError::of(line.line, error).within(line));
    if let Some(unknown) = declared
        .iter()
        .find(|dimension| !order.dimensions.contains(dimension))
    {
        return fail(OrderError::DimensionsNamesUnknown {
            unknown: unknown.clone(),
        });
    }
    if let Some(missing) = order
        .dimensions
        .iter()
        .find(|dimension| !declared.contains(dimension))
    {
        let owner = products
            .iter()
            .find(|product| product.dimensions.contains(missing))
            .map(|product| product.name.clone());
        return fail(OrderError::DimensionsLeavesOut {
            missing: missing.clone(),
            owner,
        });
    }
    for (position, first) in declared.iter().enumerate() {
        for second in &declared[position + 1..] {
            let (first, second) = (order.index(first), order.index(second));
            order.add(first, second);
        }
    }
    Ok(())
}

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
    ParseError::of(
        at.line,
        OrderError::SourcesOrderDifferently {
            name: source.name.to_string(),
            first: first.to_string(),
            second: second.to_string(),
            by: by.to_string(),
        },
    )
    .within(&at)
}

fn in_order(declared: &[String], dimensions: &[String]) -> Vec<String> {
    declared
        .iter()
        .filter(|dimension| dimensions.contains(dimension))
        .cloned()
        .collect()
}
