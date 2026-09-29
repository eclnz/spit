//! `require` and `skip` rules: checked first against the pipeline's source
//! declarations, then applied to an inventory.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{
    ArtifactInstance, CountRequirement, CoverageAction, CoverageGap, CoverageRule, EntityBinding,
    InputRules, Pipeline, SourceInventory,
};
use crate::shape::dimension_set;

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

    /// What was skipped and why, as a warning says it.
    pub(crate) fn note(&self) -> String {
        format!(
            "[{}] because `skip {}` rejected the group",
            self.context, self.target
        )
    }
}

/// Apply skip rules in declaration order. A failed group is removed from all
/// contexts and source records, so it cannot drive a downstream job.
pub(crate) fn apply_skips(
    rules: &InputRules,
    inventory: &mut SourceInventory,
    discovery_only: bool,
) -> Vec<SkippedGroup> {
    let mut skipped = Vec::new();
    for rule in &rules.constraints {
        if rule.action != CoverageAction::Skip {
            continue;
        }
        let is_discovery = rules.discovery(&rule.product).is_some();
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
                        .any(|binding| binding.get(dimension) == Some(value.as_str()))
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

/// Check that a rule names a source or discovery rule and groups by its
/// dimensions. Needs no inventory.
pub(crate) fn check_coverage_rule(
    rule_index: usize,
    rule: &CoverageRule,
    pipeline: &Pipeline,
    rules: &InputRules,
) -> Result<(), ResolveError> {
    let discovery = rules.discovery(&rule.product);
    let product = pipeline
        .products
        .iter()
        .find(|product| product.name == rule.product);
    if discovery.is_some() && product.is_some() {
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Constraint(rule_index),
            detail: format!(
                "`{}` names both a product and a discovery rule",
                rule.product
            ),
        });
    }
    let dimensions = match (discovery, product) {
        (Some(discovery), _) => &discovery.dimensions,
        (None, Some(product)) => &product.dimensions,
        (None, None) => {
            return Err(ResolveError::UnknownProduct {
                name: rule.product.clone(),
            })
        }
    };
    if discovery.is_none() && !pipeline.is_source(&rule.product) {
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

pub(crate) fn coverage_gaps(
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
            artifacts
                .get(&rule.product)
                .map_or(&[][..], Vec::as_slice)
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
                    .any(|binding| binding.get(dimension) == Some(value.as_str()))
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

/// Check every rule against the pipeline, needing no inventory, with the
/// subject each error concerns.
pub(crate) fn collect_rule_errors(
    pipeline: &Pipeline,
    rules: &InputRules,
    poisoned: &BTreeSet<String>,
) -> Vec<(DefinitionSubject, ResolveError)> {
    let mut errors = Vec::new();
    for (index, rule) in rules.constraints.iter().enumerate() {
        if poisoned.contains(&rule.product) {
            continue;
        }
        if let Err(error) = check_coverage_rule(index, rule, pipeline, rules) {
            // Keep the more specific subject a definition error names.
            let subject = match &error {
                ResolveError::InvalidDefinition { subject, .. } => subject.clone(),
                _ => DefinitionSubject::Constraint(index),
            };
            errors.push((subject, error));
        }
    }
    errors
}

/// An inventory after the skip rules, with what the `require` rules find
/// missing and the sources each gap holds back.
pub(crate) struct InputCheck {
    pub(crate) inventory: SourceInventory,
    pub(crate) gaps: Vec<CoverageGap>,
    /// The groups a `skip` rule removed from the records.
    pub(crate) skipped: Vec<SkippedGroup>,
}

/// Check an inventory's records against the pipeline's sources and its
/// named contexts against the discovery rules, apply the skip rules, and
/// find what the `require` rules miss.
pub(crate) fn check_inventory(
    pipeline: &Pipeline,
    rules: &InputRules,
    inventory: &SourceInventory,
) -> Result<InputCheck, ResolveError> {
    // A settled `.spitout` keeps the names of the rules that found its
    // contexts; without those rules the names are only a record of that.
    for (name, bindings) in &inventory.discovered {
        let Some(discovery) = rules.discovery(name) else {
            continue;
        };
        let expected: BTreeSet<_> = discovery.dimensions.iter().map(String::as_str).collect();
        for binding in bindings {
            let found: BTreeSet<_> = binding.dimensions().collect();
            if found != expected {
                return Err(ResolveError::InvalidDefinition {
                    subject: DefinitionSubject::None,
                    detail: format!(
                        "inventory context [{binding}] for discovery `{name}` must bind [{}]",
                        discovery.dimensions.join(", ")
                    ),
                });
            }
        }
    }
    // Validate every supplied record, including records a skip rule may omit.
    pipeline.source_artifacts(inventory)?;
    let mut inventory = inventory.clone();
    let skipped = apply_skips(rules, &mut inventory, false);
    let artifacts = pipeline.source_artifacts(&inventory)?;
    for (rule_index, rule) in rules.constraints.iter().enumerate() {
        if rules.discovery(&rule.product).is_some()
            && !inventory.discovered.contains_key(&rule.product)
        {
            return Err(ResolveError::InvalidDefinition {
                subject: DefinitionSubject::Constraint(rule_index),
                detail: format!(
                    "coverage rule for discovery `{}` needs named contexts in the inventory",
                    rule.product
                ),
            });
        }
    }
    let gaps = rules
        .constraints
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.action == CoverageAction::Require)
        .flat_map(|(rule_index, rule)| {
            let discovery = rules.discovery(&rule.product).is_some();
            coverage_gaps(rule_index, rule, &inventory, &artifacts, discovery)
        })
        .collect();
    Ok(InputCheck {
        inventory,
        gaps,
        skipped,
    })
}
