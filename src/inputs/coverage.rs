//! `require` and `drop` rules: checked first against the pipeline's source
//! declarations, then applied to an inventory.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::error::{DefinitionSubject, ResolveError};
use crate::model::{
    ArtifactInstance, CountRequirement, CoverageAction, CoverageGap, CoverageRule, EntityBinding,
    GroupKey, InputRules, Pipeline, PipelineIndex, Removal, SourceInventory,
};
use crate::shape::dimension_set;

/// A group a `drop` rule removed.
#[derive(Clone, Debug)]
pub(crate) struct DroppedGroup {
    pub context: EntityBinding,
    pub group_by: Vec<String>,
    /// The rule that removed it, as written, and its line.
    pub rule: String,
    pub line: Option<usize>,
    /// For a rule that counts, how many it found in the group.
    pub found: Option<usize>,
}

impl DroppedGroup {
    /// The group, as the `.spitout` records what the input stage removed.
    pub(crate) fn removal(&self) -> Removal {
        Removal {
            product: None,
            entities: self.context.clone(),
            rule: self.rule.clone(),
            origin: self.line.map(|line| format!("line {line}")),
            reason: None,
            found: self.found,
        }
    }
}

/// Dropped groups by the dimensions they group by, so a binding is tested
/// against all of them with one lookup per grouping.
pub(crate) struct DropIndex<'a>(BTreeMap<&'a [String], BTreeSet<&'a EntityBinding>>);

impl<'a> DropIndex<'a> {
    pub(crate) fn new(groups: &'a [DroppedGroup]) -> Self {
        let mut index: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for group in groups {
            index
                .entry(group.group_by.as_slice())
                .or_default()
                .insert(&group.context);
        }
        Self(index)
    }

    /// Whether `binding` lies in one of the dropped groups.
    pub(crate) fn matches(&self, binding: &EntityBinding) -> bool {
        self.0.iter().any(|(group_by, contexts)| {
            binding
                .project(group_by)
                .is_some_and(|context| contexts.contains(&context))
        })
    }
}

/// `drop` rules that would remove every group of one grouping, which leaves
/// nothing to plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EveryGroupDropped {
    pub group_by: Vec<String>,
    pub groups: usize,
    /// Each rule of that grouping, as written, with its line when known.
    pub rules: Vec<String>,
}

impl fmt::Display for EveryGroupDropped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let groups = match self.groups {
            1 => "the one".to_owned(),
            count => format!("all {count}"),
        };
        write!(
            f,
            "drop rules removed {groups} [{}] group{}, leaving nothing to plan: {}",
            self.group_by.join(", "),
            if self.groups == 1 { "" } else { "s" },
            self.rules.join(", ")
        )
    }
}

/// Judge every `drop` rule against `inventory` as it stands, then remove
/// every group any of them rejects, at once: no rule sees another's result,
/// so their order does not matter. A group two rules reject is recorded
/// with the first.
pub(crate) fn apply_drops(
    rules: &InputRules,
    inventory: &mut SourceInventory,
) -> Result<Vec<DroppedGroup>, EveryGroupDropped> {
    let mut dropped = Vec::new();
    let mut seen = FxHashSet::default();
    // Each grouping's groups, and how many of them some rule rejects.
    let mut groupings: BTreeMap<Vec<String>, (usize, usize, Vec<String>)> = BTreeMap::new();
    for rule in rules
        .constraints
        .iter()
        .filter(|rule| rule.action == CoverageAction::Drop)
    {
        let groups = all_groups(inventory, &rule.group_by);
        let members = members(rule, inventory, rules.discovery(&rule.product).is_some());
        let written = match rule.line {
            Some(line) => format!("`{rule}` (line {line})"),
            None => format!("`{rule}`"),
        };
        let entry = groupings
            .entry(rule.group_by.clone())
            .or_insert((groups.len(), 0, Vec::new()));
        entry.2.push(written);
        for (key, context) in groups {
            let members = members.get(&key).map_or(&[][..], Vec::as_slice);
            if !rule.holds_for(members) {
                continue;
            }
            if seen.insert((rule.group_by.clone(), key)) {
                entry.1 += 1;
                dropped.push(DroppedGroup {
                    context,
                    group_by: rule.group_by.clone(),
                    rule: rule.to_string(),
                    line: rule.line,
                    found: rule.count.map(|_| members.len()),
                });
            }
        }
    }
    for (group_by, (groups, rejected, rules)) in groupings {
        if groups > 0 && rejected == groups {
            return Err(EveryGroupDropped {
                group_by,
                groups,
                rules,
            });
        }
    }
    remove_groups(inventory, &dropped);
    Ok(dropped)
}

/// Each group `group_by` forms in `inventory`: every distinct value of its
/// dimensions among the artifacts and contexts, whichever source or
/// discovery found them, so a group with none of a rule's target counts 0.
fn all_groups(inventory: &SourceInventory, group_by: &[String]) -> Vec<(GroupKey, EntityBinding)> {
    let bindings = inventory
        .contexts
        .iter()
        .chain(inventory.discovered.values().flatten())
        .chain(inventory.artifacts.iter().map(|record| &record.entities));
    distinct_groups(bindings, group_by)
}

/// The bindings `rule` counts in each group: its discovery's contexts, or
/// its source's artifacts.
fn members<'a>(
    rule: &CoverageRule,
    inventory: &'a SourceInventory,
    discovery: bool,
) -> FxHashMap<GroupKey, Vec<&'a EntityBinding>> {
    let bindings: Box<dyn Iterator<Item = &EntityBinding>> = if discovery {
        Box::new(
            inventory
                .discovered
                .get(&rule.product)
                .into_iter()
                .flatten(),
        )
    } else {
        Box::new(
            inventory
                .artifacts
                .iter()
                .filter(|record| record.product == rule.product)
                .map(|record| &record.entities),
        )
    };
    let mut members: FxHashMap<GroupKey, Vec<&EntityBinding>> = FxHashMap::default();
    for binding in bindings {
        if let Some(key) = binding.group_key(&rule.group_by) {
            members.entry(key).or_default().push(binding);
        }
    }
    members
}

/// Each distinct projection of `bindings` onto `dimensions`, with its key,
/// in binding order. Bindings are grouped by key, so a projection is built
/// once for each group rather than once for each binding.
fn distinct_groups<'a>(
    bindings: impl IntoIterator<Item = &'a EntityBinding>,
    dimensions: &[String],
) -> Vec<(GroupKey, EntityBinding)> {
    let mut seen = FxHashSet::default();
    let mut groups = Vec::new();
    for binding in bindings {
        let Some(key) = binding.group_key(dimensions) else {
            continue;
        };
        if !seen.contains(&key) {
            let group = binding
                .project(dimensions)
                .expect("a binding with a key binds its dimensions");
            seen.insert(key.clone());
            groups.push((key, group));
        }
    }
    groups.sort_unstable_by(|(_, left), (_, right)| left.cmp(right));
    groups
}

/// Remove every context, discovered binding and source record in `groups`.
fn remove_groups(inventory: &mut SourceInventory, groups: &[DroppedGroup]) {
    if groups.is_empty() {
        return;
    }
    let index = DropIndex::new(groups);
    let dropped = |binding: &EntityBinding| index.matches(binding);
    inventory.contexts.retain(|binding| !dropped(binding));
    for bindings in inventory.discovered.values_mut() {
        bindings.retain(|binding| !dropped(binding));
    }
    inventory
        .artifacts
        .retain(|record| !dropped(&record.entities));
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
    pipeline: &PipelineIndex<'_>,
    rules: &InputRules,
) -> Result<(), ResolveError> {
    let discovery = rules.discovery(&rule.product);
    let product = pipeline.product(&rule.product);
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
    for (dimension, listed) in rule.values.iter().chain(&rule.has) {
        let detail = if !dimensions.contains(dimension) {
            format!(
                "coverage rule for `{}` names values of `{dimension}`, which is not a dimension of {target}",
                rule.product
            )
        } else if group_by.contains(dimension) {
            let value = listed.first().map_or("…", String::as_str);
            format!(
                "`{dimension}` is one of the rule's groups ([{}]), so each group has one `{dimension}`; \
                 a value clause checks another dimension within each group. \
                 To remove named groups, write `exclude [{dimension}={value}]`",
                rule.group_by.join(", ")
            )
        } else {
            continue;
        };
        return Err(ResolveError::InvalidDefinition {
            subject: DefinitionSubject::Constraint(rule_index),
            detail,
        });
    }
    Ok(())
}

/// What `require` rule `rule` finds wrong with `inventory`: each group
/// that fails it, with the sources each holds back, or, when the rule's
/// grouping forms no group at all, that it has nothing to check.
pub(crate) fn coverage_gaps(
    rule_index: usize,
    rule: &CoverageRule,
    inventory: &SourceInventory,
    artifacts: &BTreeMap<String, Vec<ArtifactInstance>>,
    discovery: bool,
) -> Vec<CoverageGap> {
    let groups = all_groups(inventory, &rule.group_by);
    if groups.is_empty() {
        return vec![CoverageGap {
            error: ResolveError::NoGroupsToCheck {
                rule: rule.to_string(),
                rule_index,
            },
            sources: Vec::new(),
        }];
    }
    // Each group's members, found in one pass rather than once per group.
    let mut sources: FxHashMap<GroupKey, Vec<&ArtifactInstance>> = FxHashMap::default();
    let bindings = if discovery {
        members(rule, inventory, true)
    } else {
        let mut bindings: FxHashMap<GroupKey, Vec<&EntityBinding>> = FxHashMap::default();
        for artifact in artifacts.get(&rule.product).into_iter().flatten() {
            if let Some(key) = artifact.entities.group_key(&rule.group_by) {
                sources.entry(key.clone()).or_default().push(artifact);
                bindings.entry(key).or_default().push(&artifact.entities);
            }
        }
        bindings
    };
    let mut gaps = Vec::new();
    for (key, context) in groups {
        let held = sources.get(&key).map_or(&[][..], Vec::as_slice);
        let bindings = bindings.get(&key).map_or(&[][..], Vec::as_slice);
        let errors = group_errors(rule_index, rule, &context, bindings, discovery);
        gaps.extend(errors.into_iter().map(|error| CoverageGap {
            error,
            sources: held.iter().map(|&member| member.clone()).collect(),
        }));
    }
    gaps
}

/// What `rule` finds wrong with one group, whose members have `bindings`:
/// a count that does not hold, and each required value none of them has.
fn group_errors(
    rule_index: usize,
    rule: &CoverageRule,
    context: &EntityBinding,
    bindings: &[&EntityBinding],
    discovery: bool,
) -> Vec<ResolveError> {
    let found = bindings.len();
    let expected = rule.count.unwrap_or(CountRequirement::AtLeast(1));
    let mut errors = Vec::new();
    if !expected.allows(found) {
        errors.push(ResolveError::CoverageViolation {
            product: rule.product.clone(),
            rule_index,
            context: context.clone(),
            expected,
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
    // Found once: each rule asks which products are sources and which is
    // named.
    let products = PipelineIndex::new(pipeline);
    let mut errors = super::exclusions::collect_exclusion_errors(&products, rules);
    for (index, rule) in rules.constraints.iter().enumerate() {
        if poisoned.contains(&rule.product) {
            continue;
        }
        if let Err(error) = check_coverage_rule(index, rule, &products, rules) {
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

/// An inventory with what the `require` rules find missing, and the sources
/// each gap holds back.
pub(crate) struct InputCheck<'a> {
    /// The inventory as given: checking it changes nothing.
    pub(crate) inventory: Cow<'a, SourceInventory>,
    pub(crate) gaps: Vec<CoverageGap>,
}

/// Check an inventory's records against the pipeline's sources and its
/// named contexts against the discovery rules, and find what the `require`
/// rules miss. `drop` rules have been applied already, where the inventory
/// was settled, so each rule is judged once against what they leave.
pub(crate) fn check_inventory<'a>(
    pipeline: &Pipeline,
    rules: &InputRules,
    inventory: Cow<'a, SourceInventory>,
) -> Result<InputCheck<'a>, ResolveError> {
    check_discovered(rules, &inventory)?;
    let requires = rules
        .constraints
        .iter()
        .any(|rule| rule.action == CoverageAction::Require);
    // Validate every supplied record. Only `require` rules need the records
    // as artifacts.
    let artifacts = if requires {
        pipeline.source_artifacts(&inventory)?
    } else {
        check_records(pipeline, &inventory)?;
        BTreeMap::new()
    };
    check_named_contexts(rules, &inventory)?;
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
    Ok(InputCheck { inventory, gaps })
}

/// Check records given directly, and their named contexts, before any rule
/// removes some, so a removed group cannot hide a mistake in them.
pub(crate) fn check_before_removal(
    pipeline: &Pipeline,
    rules: &InputRules,
    inventory: &SourceInventory,
) -> Result<(), ResolveError> {
    check_discovered(rules, inventory)?;
    check_records(pipeline, inventory)
}

/// Check that every record names a source with its dimensions, once each.
fn check_records(pipeline: &Pipeline, inventory: &SourceInventory) -> Result<(), ResolveError> {
    let mut seen = FxHashSet::default();
    pipeline.check_sources(inventory, |_, record| {
        seen.insert((record.product.as_str(), &record.entities))
    })
}

/// A settled `.spitout` keeps the names of the rules that found its
/// contexts; each context of a rule the recipe has must bind its dimensions.
/// Without that rule the names are only a record of where they came from.
fn check_discovered(rules: &InputRules, inventory: &SourceInventory) -> Result<(), ResolveError> {
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
    Ok(())
}

/// Each rule on a discovery needs that discovery's contexts named in the
/// inventory.
fn check_named_contexts(
    rules: &InputRules,
    inventory: &SourceInventory,
) -> Result<(), ResolveError> {
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
    Ok(())
}
