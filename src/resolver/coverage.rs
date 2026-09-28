//! Coverage rules: the sources each rule requires, checked first against
//! the pipeline and then against an inventory.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{
    ArtifactInstance, CountRequirement, CoverageGap, CoverageRule, ProductDef, SourceInventory,
};
use crate::shape::dimension_set;

use super::{family, find_product};

pub(super) fn check_coverage_rule(
    rule_index: usize,
    rule: &CoverageRule,
    products: &BTreeMap<&str, &ProductDef>,
    producers: &BTreeMap<String, usize>,
) -> Result<(), ResolveError> {
    let product = find_product(products, &rule.product)?;
    if producers.contains_key(&rule.product) {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Constraint(rule_index),
            detail: format!(
                "coverage rule product `{}` must be a source family",
                rule.product
            ),
        });
    }
    let group_by = dimension_set(&rule.group_by);
    if group_by.len() != rule.group_by.len()
        || !group_by.is_subset(&dimension_set(&product.dimensions))
    {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::ConstraintGroup(rule_index),
            detail: format!(
                "coverage rule for `{}` must group by distinct dimensions of that product",
                rule.product
            ),
        });
    }
    for dimension in rule.values.keys() {
        if !product.dimensions.contains(dimension) || group_by.contains(dimension) {
            return Err(ResolveError::InvalidDefinition {
                subject: DefinitionSubject::Constraint(rule_index),
                detail: format!(
                    "coverage rule for `{}` requires values of `{dimension}`, which must be a dimension of that product outside its groups",
                    rule.product
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn coverage_gaps(
    rule_index: usize,
    rule: &CoverageRule,
    inventory: &SourceInventory,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
) -> Vec<CoverageGap> {
    let groups: BTreeSet<_> = inventory
        .contexts
        .iter()
        .chain(inventory.artifacts.iter().map(|record| &record.entities))
        .filter_map(|binding| binding.project(&rule.group_by))
        .collect();
    let mut gaps = Vec::new();
    for context in groups {
        let members: Vec<_> = family(artifacts, &rule.product)
            .iter()
            .filter(|artifact| artifact.entities.project(&rule.group_by).as_ref() == Some(&context))
            .cloned()
            .collect();
        let found = members.len();
        let valid = match rule.count {
            CountRequirement::Exactly(expected) => found == expected,
            CountRequirement::AtLeast(minimum) => found >= minimum,
        };
        let mut errors = Vec::new();
        if !valid {
            errors.push(ResolveError::CoverageViolation {
                product: rule.product.clone(),
                rule_index,
                context: context.clone(),
                expected: rule.count.clone(),
                found,
            });
        }
        for (dimension, values) in &rule.values {
            let missing = values.iter().filter(|value| {
                !members
                    .iter()
                    .any(|artifact| artifact.entities.0.get(dimension) == Some(*value))
            });
            errors.extend(missing.map(|value| ResolveError::MissingRequiredValue {
                product: rule.product.clone(),
                rule_index,
                context: context.clone(),
                dimension: dimension.clone(),
                value: value.clone(),
            }));
        }
        gaps.extend(errors.into_iter().map(|error| CoverageGap {
            error,
            sources: members.clone(),
        }));
    }
    gaps
}
