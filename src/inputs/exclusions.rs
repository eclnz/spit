//! `exclude` rules: removing named artifacts and groups from a dataset,
//! and recording what each removed.

use std::borrow::Cow;
use std::collections::BTreeSet;

use crate::model::{
    near_reason, EntityBinding, Exclusion, InputRules, PipelineIndex, Removal, SourceInventory,
};
use crate::model::{DefinitionSubject, ResolveError};

/// Check each rule against the pipeline without reading any data: a rule's
/// product must be a source, and each dimension it names must be one of that
/// source's or, for a rule naming no source, of some source.
pub(crate) fn collect_exclusion_errors(
    products: &PipelineIndex<'_>,
    rules: &InputRules,
) -> Vec<(DefinitionSubject, ResolveError)> {
    let pipeline = products.pipeline;
    let mut errors = Vec::new();
    // The dimensions of every source, for the rules that name none.
    let all_dimensions: Vec<&str> = pipeline
        .products
        .iter()
        .filter(|product| products.is_source(&product.name))
        .flat_map(|product| product.dimensions.iter().map(String::as_str))
        .collect();
    for (index, rule) in rules.exclusions.iter().enumerate() {
        let problem = |detail: String| ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Exclusion(index),
            detail: format!("`{rule}`: {detail}"),
        };
        let dimensions: Cow<'_, [&str]> = match &rule.product {
            Some(name) => {
                if !products.is_source(name) {
                    errors.push((
                        DefinitionSubject::Exclusion(index),
                        problem(format!("`{name}` is not a source")),
                    ));
                    continue;
                }
                Cow::Owned(
                    pipeline
                        .products
                        .iter()
                        .filter(|product| &product.name == name)
                        .flat_map(|product| product.dimensions.iter().map(String::as_str))
                        .collect(),
                )
            }
            None => Cow::Borrowed(all_dimensions.as_slice()),
        };
        for (dimension, _) in &rule.values {
            if !dimensions.contains(&dimension.as_str()) {
                let owner = rule
                    .product
                    .as_ref()
                    .map_or_else(|| "any source".to_owned(), |name| format!("`{name}`"));
                errors.push((
                    DefinitionSubject::Exclusion(index),
                    problem(format!("`{dimension}` is not a dimension of {owner}")),
                ));
            }
        }
    }
    errors
}

/// Applies `exclude` rules to what the input stage finds, one artifact or
/// context at a time, and records what each removes.
pub(crate) struct Excluder<'a> {
    rules: &'a [Exclusion],
    matched: Vec<bool>,
    /// Whether a rule that names no source has recorded its group.
    recorded: Vec<bool>,
    removed: Vec<(usize, Removal)>,
    /// Values close to those each rule names, seen where it matched nothing.
    near: Vec<BTreeSet<(String, String)>>,
}

impl<'a> Excluder<'a> {
    pub(crate) fn new(rules: &'a [Exclusion]) -> Self {
        Self {
            rules,
            matched: vec![false; rules.len()],
            recorded: vec![false; rules.len()],
            removed: Vec::new(),
            near: vec![BTreeSet::new(); rules.len()],
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether a rule removes the artifact of `product` with `entities`,
    /// recording it with the first rule that does.
    pub(crate) fn artifact(&mut self, product: &str, entities: &EntityBinding) -> bool {
        let mut first = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.matches(product, entities) {
                self.matched[index] = true;
                first.get_or_insert(index);
            } else if !self.matched[index] {
                note_near(&mut self.near[index], rule, Some(product), entities);
            }
        }
        let Some(index) = first else {
            return false;
        };
        self.record(index, Some(product), entities);
        true
    }

    /// Whether a rule that names no source removes the discovered context
    /// `binding`, recording it.
    pub(crate) fn context(&mut self, binding: &EntityBinding) -> bool {
        let mut first = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.matches_context(binding) {
                self.matched[index] = true;
                first.get_or_insert(index);
            } else if !self.matched[index] {
                note_near(&mut self.near[index], rule, None, binding);
            }
        }
        let Some(index) = first else {
            return false;
        };
        self.record(index, None, binding);
        true
    }

    /// Remove every record and context of `inventory` a rule names.
    pub(crate) fn apply(&mut self, inventory: &mut SourceInventory) {
        if self.is_empty() {
            return;
        }
        inventory
            .artifacts
            .retain(|record| !self.artifact(&record.product, &record.entities));
        inventory.contexts.retain(|binding| !self.context(binding));
    }

    /// A rule that names a source records each artifact it removes; one
    /// that names none records its group once.
    fn record(&mut self, index: usize, product: Option<&str>, entities: &EntityBinding) {
        let rule = &self.rules[index];
        let (product, entities) = match &rule.product {
            Some(_) => match product {
                Some(product) => (Some(product.to_owned()), entities.clone()),
                None => return,
            },
            None if self.recorded[index] => return,
            None => {
                self.recorded[index] = true;
                (None, rule.binding())
            }
        };
        self.removed.push((
            index,
            Removal {
                product,
                entities,
                rule: rule.to_string(),
                origin: Some(rule.origin.clone()),
                reason: rule.reason.clone(),
                found: None,
            },
        ));
    }

    /// What the rules removed, rule by rule, or, for the first rule that
    /// removed nothing, the error.
    pub(crate) fn finish(mut self) -> Result<Vec<Removal>, UnmatchedExclusion> {
        if let Some(index) = self.matched.iter().position(|matched| !matched) {
            let rule = &self.rules[index];
            let near: Vec<_> = self.near[index]
                .iter()
                .map(|(dimension, value)| format!("{dimension}={value}"))
                .collect();
            return Err(UnmatchedExclusion {
                rule: rule.to_string(),
                origin: rule.origin.clone(),
                near,
            });
        }
        self.removed
            .sort_by(|(left_rule, left), (right_rule, right)| {
                left_rule
                    .cmp(right_rule)
                    .then_with(|| left.product.cmp(&right.product))
                    .then_with(|| left.entities.cmp_in(&right.entities, &[]))
            });
        Ok(self
            .removed
            .into_iter()
            .map(|(_, removal)| removal)
            .collect())
    }
}

/// An `exclude` rule that matched nothing in the dataset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnmatchedExclusion {
    pub rule: String,
    pub origin: String,
    /// Values the dataset has that differ from the rule's only in letter case
    /// or leading zeros, as `sub=02`.
    pub near: Vec<String>,
}

/// Note each value of `entities` that is close to, but not, a value `rule`
/// names, where the rest of the rule could match.
fn note_near(
    near: &mut BTreeSet<(String, String)>,
    rule: &Exclusion,
    product: Option<&str>,
    entities: &EntityBinding,
) {
    if rule
        .product
        .as_deref()
        .is_some_and(|name| Some(name) != product)
    {
        return;
    }
    for (dimension, value) in &rule.values {
        if let Some(found) = entities.get(dimension) {
            if near_reason(found, value).is_some() {
                near.insert((dimension.to_owned(), found.to_owned()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_values_differ_in_case_or_leading_zeros() {
        assert!(near_reason("S07", "s07").is_some());
        assert!(near_reason("02", "2").is_some());
        assert!(near_reason("03", "2").is_none());
        assert!(near_reason("run-2", "run-02").is_none());
    }
}
