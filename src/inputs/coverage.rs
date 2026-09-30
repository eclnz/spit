//! `require` and `skip` rules: checked first against the pipeline's source
//! declarations, then applied to an inventory.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{
    ArtifactInstance, CoverageAction, CoverageGap, CoverageRule, EntityBinding, InputRules,
    Pipeline, SourceInventory,
};
use crate::shape::dimension_set;

#[derive(Clone, Debug)]
pub(crate) struct SkippedGroup {
    pub target: String,
    pub context: EntityBinding,
    pub group_by: Vec<String>,
}

impl SkippedGroup {
    /// What was skipped and why, as a warning says it.
    pub(crate) fn note(&self) -> String {
        format!(
            "[{}] because `skip {}` rejected the group",
            self.context, self.target
        )
    }
}

/// Skipped groups by the dimensions they group by, so a binding is tested
/// against all of them with one lookup per grouping.
pub(crate) struct SkipIndex<'a>(BTreeMap<&'a [String], BTreeSet<&'a EntityBinding>>);

impl<'a> SkipIndex<'a> {
    pub(crate) fn new(groups: &'a [SkippedGroup]) -> Self {
        let mut index: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for group in groups {
            index
                .entry(group.group_by.as_slice())
                .or_default()
                .insert(&group.context);
        }
        Self(index)
    }

    /// Whether `binding` lies in one of the skipped groups.
    pub(crate) fn matches(&self, binding: &EntityBinding) -> bool {
        self.0.iter().any(|(group_by, contexts)| {
            binding
                .project(group_by)
                .is_some_and(|context| contexts.contains(&context))
        })
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
        let rejected = rejected_groups(rule, inventory, is_discovery);
        remove_groups(inventory, &rejected);
        skipped.extend(rejected);
    }
    skipped
}

/// The groups of `inventory` that skip `rule` rejects: too few or too many
/// members, or a required value missing.
fn rejected_groups(
    rule: &CoverageRule,
    inventory: &SourceInventory,
    is_discovery: bool,
) -> Vec<SkippedGroup> {
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
    // Each group's members, found in one pass rather than once per group.
    let mut members: BTreeMap<EntityBinding, Vec<&EntityBinding>> = BTreeMap::new();
    for binding in bindings {
        if let Some(group) = binding.project(&rule.group_by) {
            members.entry(group).or_default().push(binding);
        }
    }
    groups
        .into_iter()
        .filter(|context| {
            let members = members.get(context).map_or(&[][..], Vec::as_slice);
            !rule.count.allows(members.len()) || missing_values(rule, members).next().is_some()
        })
        .map(|context| SkippedGroup {
            target: rule.product.clone(),
            context,
            group_by: rule.group_by.clone(),
        })
        .collect()
}

/// Remove every context, discovered binding and source record in `groups`.
fn remove_groups(inventory: &mut SourceInventory, groups: &[SkippedGroup]) {
    if groups.is_empty() {
        return;
    }
    let index = SkipIndex::new(groups);
    let skipped = |binding: &EntityBinding| index.matches(binding);
    inventory.contexts.retain(|binding| !skipped(binding));
    for bindings in inventory.discovered.values_mut() {
        bindings.retain(|binding| !skipped(binding));
    }
    inventory
        .artifacts
        .retain(|record| !skipped(&record.entities));
}

/// Each value `rule` requires, by dimension, that no binding in a group has.
fn missing_values<'a>(
    rule: &'a CoverageRule,
    bindings: &'a [&EntityBinding],
) -> impl Iterator<Item = (&'a String, &'a String)> + 'a {
    rule.values.iter().flat_map(move |(dimension, values)| {
        values
            .iter()
            .filter(move |value| {
                !bindings
                    .iter()
                    .any(|binding| binding.get(dimension) == Some(value.as_str()))
            })
            .map(move |value| (dimension, value))
    })
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
    // Each group's members, found in one pass rather than once per group.
    let mut members: BTreeMap<EntityBinding, Vec<ArtifactInstance>> = BTreeMap::new();
    let mut bindings: BTreeMap<EntityBinding, Vec<&EntityBinding>> = BTreeMap::new();
    if discovery {
        for binding in discovered.into_iter().flatten() {
            if let Some(group) = binding.project(&rule.group_by) {
                bindings.entry(group).or_default().push(binding);
            }
        }
    } else {
        for artifact in artifacts.get(&rule.product).into_iter().flatten() {
            if let Some(group) = artifact.entities.project(&rule.group_by) {
                members.entry(group).or_default().push(artifact.clone());
            }
        }
        for (group, artifacts) in &members {
            let entities = artifacts.iter().map(|artifact| &artifact.entities);
            bindings.insert(group.clone(), entities.collect());
        }
    }
    let mut gaps = Vec::new();
    for context in groups {
        let members = members.get(&context).map_or(&[][..], Vec::as_slice);
        let bindings = bindings.get(&context).map_or(&[][..], Vec::as_slice);
        let errors = group_errors(rule_index, rule, &context, bindings, discovery);
        gaps.extend(errors.into_iter().map(|error| CoverageGap {
            error,
            sources: members.to_vec(),
        }));
    }
    gaps
}

/// What `rule` finds wrong with one group, whose members have `bindings`:
/// too few or too many, and each required value none of them has.
fn group_errors(
    rule_index: usize,
    rule: &CoverageRule,
    context: &EntityBinding,
    bindings: &[&EntityBinding],
    discovery: bool,
) -> Vec<ResolveError> {
    let found = bindings.len();
    let mut errors = Vec::new();
    if !rule.count.allows(found) {
        errors.push(ResolveError::CoverageViolation {
            product: rule.product.clone(),
            rule_index,
            context: context.clone(),
            expected: rule.count.clone(),
            found,
            discovery,
        });
    }
    errors.extend(missing_values(rule, bindings).map(|(dimension, value)| {
        ResolveError::MissingRequiredValue {
            product: rule.product.clone(),
            rule_index,
            context: context.clone(),
            dimension: dimension.clone(),
            value: value.clone(),
            discovery,
        }
    }));
    errors
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
    let supplied = pipeline.source_artifacts(inventory)?;
    let mut inventory = inventory.clone();
    let skipped = apply_skips(rules, &mut inventory, false);
    // Skipping only removes records, so when none go the artifacts stand.
    let artifacts = if inventory.artifacts.len() == supplied.values().map(Vec::len).sum::<usize>() {
        supplied
    } else {
        pipeline.source_artifacts(&inventory)?
    };
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
