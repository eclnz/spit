//! Coverage rules: the sources each rule requires, checked first against
//! the pipeline and then against an inventory.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{
    ArtifactInstance, CountRequirement, CoverageAction, CoverageGap, CoverageRule,
    DirectoryDiscovery, EntityBinding, ProductDef, SourceInventory,
};
use crate::shape::dimension_set;

use super::{family, find_product};

#[derive(Clone, Debug)]
pub(crate) struct SkippedGroup {
    pub target: String,
    pub context: EntityBinding,
    pub group_by: Vec<String>,
}

impl SkippedGroup {
    pub(crate) fn matches(&self, binding: &EntityBinding) -> bool {
        binding.project(&self.group_by).as_ref() == Some(&self.context)
    }
}

/// Apply skip rules in declaration order. A failed group is removed from all
/// contexts and source records, so it cannot drive a downstream job.
pub(crate) fn apply_skips(
    pipeline: &crate::model::Pipeline,
    inventory: &mut SourceInventory,
    discovery_only: bool,
) -> Vec<SkippedGroup> {
    let mut skipped = Vec::new();
    for rule in &pipeline.constraints {
        if rule.action != CoverageAction::Skip {
            continue;
        }
        let is_discovery = pipeline
            .discoveries
            .iter()
            .any(|item| item.name == rule.product);
        if discovery_only && !is_discovery {
            continue;
        }
        let bindings: Vec<_> = if is_discovery {
            inventory
                .discovered
                .get(&rule.product)
                .into_iter()
                .flatten()
                .collect()
        } else {
            inventory
                .artifacts
                .iter()
                .filter(|record| record.product == rule.product)
                .map(|record| &record.entities)
                .collect()
        };
        let groups: BTreeSet<_> = if is_discovery {
            bindings
                .iter()
                .filter_map(|binding| binding.project(&rule.group_by))
                .collect()
        } else {
            inventory
                .contexts
                .iter()
                .chain(inventory.artifacts.iter().map(|record| &record.entities))
                .filter_map(|binding| binding.project(&rule.group_by))
                .collect()
        };
        for context in groups {
            let members: Vec<_> = bindings
                .iter()
                .filter(|binding| binding.project(&rule.group_by).as_ref() == Some(&context))
                .collect();
            let valid_count = match rule.count {
                CountRequirement::Exactly(n) => members.len() == n,
                CountRequirement::AtLeast(n) => members.len() >= n,
            };
            let valid_values = rule.values.iter().all(|(dimension, values)| {
                values.iter().all(|value| {
                    members
                        .iter()
                        .any(|binding| binding.0.get(dimension) == Some(value))
                })
            });
            if !valid_count || !valid_values {
                skipped.push(SkippedGroup {
                    target: rule.product.clone(),
                    context,
                    group_by: rule.group_by.clone(),
                });
            }
        }
        let current: Vec<_> = skipped
            .iter()
            .filter(|group| group.target == rule.product)
            .collect();
        if !current.is_empty() {
            inventory
                .contexts
                .retain(|binding| !current.iter().any(|group| group.matches(binding)));
            for bindings in inventory.discovered.values_mut() {
                bindings.retain(|binding| !current.iter().any(|group| group.matches(binding)));
            }
            inventory
                .artifacts
                .retain(|record| !current.iter().any(|group| group.matches(&record.entities)));
        }
    }
    skipped
}

pub(super) fn check_coverage_rule(
    rule_index: usize,
    rule: &CoverageRule,
    products: &BTreeMap<&str, &ProductDef>,
    producers: &BTreeMap<String, usize>,
    discoveries: &[DirectoryDiscovery],
) -> Result<(), ResolveError> {
    let discovery = discoveries
        .iter()
        .find(|candidate| candidate.name == rule.product);
    if discovery.is_some() && products.contains_key(rule.product.as_str()) {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Constraint(rule_index),
            detail: format!(
                "`{}` names both a product and a discovery rule",
                rule.product
            ),
        });
    }
    let dimensions = if let Some(discovery) = discovery {
        &discovery.dimensions
    } else {
        &find_product(products, &rule.product)?.dimensions
    };
    if discovery.is_none() && producers.contains_key(&rule.product) {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Constraint(rule_index),
            detail: format!(
                "coverage rule product `{}` must be a source family",
                rule.product
            ),
        });
    }
    let group_by = dimension_set(&rule.group_by);
    let target = if discovery.is_some() {
        "that discovery rule"
    } else {
        "that product"
    };
    if group_by.len() != rule.group_by.len() || !group_by.is_subset(&dimension_set(dimensions)) {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::ConstraintGroup(rule_index),
            detail: format!(
                "coverage rule for `{}` must group by distinct dimensions of {target}",
                rule.product
            ),
        });
    }
    for dimension in rule.values.keys() {
        if !dimensions.contains(dimension) || group_by.contains(dimension) {
            return Err(ResolveError::InvalidDefinition {
                subject: DefinitionSubject::Constraint(rule_index),
                detail: format!(
                    "coverage rule for `{}` requires values of `{dimension}`, which must be a dimension of {target} outside its groups",
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
    discovery: bool,
) -> Vec<CoverageGap> {
    let discovered = inventory.discovered.get(&rule.product);
    let groups: BTreeSet<_> = if discovery {
        discovered
            .into_iter()
            .flatten()
            .filter_map(|binding| binding.project(&rule.group_by))
            .collect()
    } else {
        inventory
            .contexts
            .iter()
            .chain(inventory.discovered.values().flatten())
            .chain(inventory.artifacts.iter().map(|record| &record.entities))
            .filter_map(|binding| binding.project(&rule.group_by))
            .collect()
    };
    let mut gaps = Vec::new();
    for context in groups {
        let members: Vec<_> = if discovery {
            Vec::new()
        } else {
            family(artifacts, &rule.product)
                .iter()
                .filter(|artifact| {
                    artifact.entities.project(&rule.group_by).as_ref() == Some(&context)
                })
                .cloned()
                .collect()
        };
        let bindings: Vec<_> = if discovery {
            discovered
                .into_iter()
                .flatten()
                .filter(|binding| binding.project(&rule.group_by).as_ref() == Some(&context))
                .collect()
        } else {
            members.iter().map(|artifact| &artifact.entities).collect()
        };
        let found = bindings.len();
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
                discovery,
            });
        }
        for (dimension, values) in &rule.values {
            let missing = values.iter().filter(|value| {
                !bindings
                    .iter()
                    .any(|binding| binding.0.get(dimension) == Some(*value))
            });
            errors.extend(missing.map(|value| ResolveError::MissingRequiredValue {
                product: rule.product.clone(),
                rule_index,
                context: context.clone(),
                dimension: dimension.clone(),
                value: value.clone(),
                discovery,
            }));
        }
        gaps.extend(errors.into_iter().map(|error| CoverageGap {
            error,
            sources: members.clone(),
        }));
    }
    gaps
}
