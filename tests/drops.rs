//! `drop` rules: removing the groups that meet a condition, all judged
//! against the same inventory, and the `require` rules checked after them.

mod support;

use support::Tree;

use spit::{
    parse_input_spec, parse_pipeline, parse_source_inventory, InputError, InputSource,
    ResolveError, ResolvedInputs,
};

const PIPELINE: &str = "\
source bold : Bold [sub, ses, run]
path bold: sub-{sub}/ses-{ses}/run-{run}.nii
source t1w : T1 [sub, ses]
path t1w: sub-{sub}/ses-{ses}/t1w.nii
operation align(bold: Bold, t1w: T1) -> Bold
aligned = align(bold, t1w)
";

/// Subject 1 has two full sessions; subject 2 one session with three runs;
/// subject 3 a session with runs but no T1w.
const RECORDS: &str = "\
sources:
    bold[sub=1,ses=1,run=1]
    bold[sub=1,ses=1,run=2]
    bold[sub=1,ses=2,run=1]
    bold[sub=1,ses=2,run=2]
    bold[sub=2,ses=1,run=1]
    bold[sub=2,ses=1,run=2]
    bold[sub=2,ses=1,run=3]
    bold[sub=3,ses=1,run=1]
    t1w[sub=1,ses=1]
    t1w[sub=1,ses=2]
    t1w[sub=2,ses=1]
";

/// The input stage over `RECORDS` with the recipe `rules`.
fn settle(rules: &str) -> Result<ResolvedInputs, InputError> {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let records = parse_source_inventory(RECORDS).unwrap();
    parse_input_spec(rules)
        .unwrap()
        .resolve(&pipeline, InputSource::Inventory(records))
}

/// What the stage removed, as `[sub=3] by rule; found n`.
fn removed(settled: &ResolvedInputs) -> Vec<String> {
    settled
        .inventory
        .removed
        .iter()
        .map(|removal| match removal.found {
            Some(found) => format!("{} by {}; found {found}", removal.identity(), removal.rule),
            None => format!("{} by {}", removal.identity(), removal.rule),
        })
        .collect()
}

#[test]
fn a_drop_rule_names_its_groups_and_the_condition_that_removes_them() {
    for (rule, written) in [
        (
            "drop [sub] where t1w count<2",
            "drop [sub] where t1w count<2",
        ),
        (
            "drop [sub, ses] where bold count>=3",
            "drop [sub, ses] where bold count>=3",
        ),
        (
            "drop [sub,ses] where bold count!=2",
            "drop [sub, ses] where bold count!=2",
        ),
        (
            "drop [sub] where t1w count=0",
            "drop [sub] where t1w count=0",
        ),
        (
            "drop [sub, ses] where bold missing run=2",
            "drop [sub, ses] where bold missing run=2",
        ),
        (
            "drop [sub, ses] where bold has run=3",
            "drop [sub, ses] where bold has run=3",
        ),
    ] {
        let spec = parse_input_spec(&format!("{rule}\n")).unwrap();
        assert_eq!(spec.rules.constraints[0].to_string(), written, "{rule}");
    }
    for (rule, message) in [
        (
            "drop sub where t1w count<2",
            "expected `drop [dimensions] where source`",
        ),
        (
            "drop [sub] t1w count<2",
            "expected `where` after the groups",
        ),
        ("drop [sub] where t1w", "one condition"),
        (
            "drop [sub] where t1w count<2 has run=1",
            "a `drop` rule takes one condition",
        ),
        (
            "drop [sub] where t1w missing",
            "expected values after `missing`",
        ),
        ("drop [sub] where t1w count<two", "nonnegative integer"),
    ] {
        let error = parse_input_spec(&format!("{rule}\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{rule}: {error}");
    }
}

#[test]
fn a_skip_rule_is_an_error_that_names_the_drop_rule_to_write() {
    for (skip, drop) in [
        (
            "skip sessions count>=2 per [sub]",
            "drop [sub] where sessions count<2",
        ),
        (
            "skip image count=1 per [sub, ses]",
            "drop [sub, ses] where image count!=1",
        ),
        (
            "skip image run=1,2 per [sub]",
            "drop [sub] where image missing run=1,2",
        ),
    ] {
        let error = parse_input_spec(&format!("{skip}\n"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!(
                "`skip` is replaced by `drop`, which names the groups to remove: write `{drop}`"
            )),
            "{skip}: {error}"
        );
    }
}

#[test]
fn a_rule_in_the_other_rules_order_is_an_error_that_names_it_in_its_own() {
    let require = "`require` names the groups first, then `where`, as `drop` does";
    let drop = "`drop` names the groups first, then `where`";
    for (rule, message) in [
        (
            "require t1w count=1 per [sub, ses]",
            format!("{require}: write `require [sub, ses] where t1w count=1`"),
        ),
        (
            "require bold run=1,2 per [sub]",
            format!("{require}: write `require [sub] where bold has run=1,2`"),
        ),
        (
            "require bold count>=2 run=1,2 per [sub]",
            format!("{require}: write `require [sub] where bold count>=2 has run=1,2`"),
        ),
        (
            "drop t1w count=0 per [sub, ses]",
            format!("{drop}: write `drop [sub, ses] where t1w count=0`"),
        ),
        (
            "drop bold run=3 per [sub, ses]",
            format!(
                "{drop}: write `drop [sub, ses] where bold has run=3` to remove the groups \
                 that have them, or `drop [sub, ses] where bold missing run=3` to remove the \
                 groups that lack one"
            ),
        ),
        (
            "drop bold count=1 run=3 per [sub]",
            format!("{drop}, as in `drop [sub] where sessions count<2`"),
        ),
    ] {
        let error = parse_input_spec(&format!("{rule}\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(&message), "{rule}: {error}");
    }
}

#[test]
fn a_group_with_none_of_the_target_counts_zero() {
    // Subject 3 has runs but no T1w, so its group holds no T1w.
    let settled = settle("drop [sub] where t1w count=0\n").unwrap();
    assert_eq!(
        removed(&settled),
        ["[sub=3] by drop [sub] where t1w count=0; found 0"]
    );
    assert!(!settled
        .inventory
        .artifacts
        .iter()
        .any(|record| record.entities.get("sub") == Some("3")));
}

#[test]
fn rules_are_judged_together_so_their_order_does_not_matter() {
    // The session rule removes subject 2's only session, and would leave it
    // with no sessions; the subject rule, judged on the same inventory,
    // still sees one session and removes subject 3 only.
    let sessions = "drop [sub, ses] where bold has run=3\n";
    let subjects = "drop [sub] where t1w count<1\n";
    let forward = settle(&format!("{sessions}{subjects}")).unwrap();
    let backward = settle(&format!("{subjects}{sessions}")).unwrap();
    assert_eq!(forward.inventory.artifacts, backward.inventory.artifacts);
    let mut kept: Vec<_> = forward
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.get("sub").unwrap().to_owned())
        .collect();
    kept.dedup();
    assert_eq!(kept, ["1"]);
    assert_eq!(forward.inventory.removed.len(), 2);
}

#[test]
fn a_group_two_rules_remove_is_recorded_once() {
    let settled = settle("drop [sub] where t1w count<1\ndrop [sub] where bold count<2\n").unwrap();
    assert_eq!(
        removed(&settled),
        ["[sub=3] by drop [sub] where t1w count<1; found 0"]
    );
}

#[test]
fn removing_every_group_is_an_error() {
    let error = settle("drop [sub] where bold count>=1\n").unwrap_err();
    assert!(matches!(error, InputError::EveryGroupDropped(_)), "{error}");
    assert_eq!(
        error.to_string(),
        "drop rules removed all 3 [sub] groups, leaving nothing to plan: \
         `drop [sub] where bold count>=1` (line 1)"
    );
    // Two rules between them, each removing some.
    let error =
        settle("drop [sub] where t1w count<1\ndrop [sub] where t1w count>=1\n").unwrap_err();
    assert!(
        error.to_string().contains("removed all 3 [sub] groups"),
        "{error}"
    );
}

#[test]
fn require_is_checked_after_the_drops_and_never_over_nothing() {
    // After subject 3 is dropped, every subject has a T1w per session.
    let settled =
        settle("drop [sub] where t1w count<1\nrequire [sub, ses] where t1w count=1\n").unwrap();
    assert!(settled.gaps.is_empty(), "{:?}", settled.gaps);
    // Without the drop, subject 3's session lacks its T1w.
    let settled = settle("require [sub, ses] where t1w count=1\n").unwrap();
    assert_eq!(settled.gaps.len(), 1);
    // A grouping no record has forms no group to check.
    let pipeline = parse_pipeline("source x [a]\nsource y [b]\n").unwrap();
    let records = parse_source_inventory("sources:\n    y[b=1]\n").unwrap();
    let settled = parse_input_spec("require [a] where x count>=1\n")
        .unwrap()
        .resolve(&pipeline, InputSource::Inventory(records))
        .unwrap();
    assert!(matches!(
        settled.require_complete(),
        Err(ResolveError::NoGroupsToCheck { .. })
    ));
    assert_eq!(
        settled.require_complete().unwrap_err().to_string(),
        "`require [a] where x count>=1` has no groups to check: nothing in the dataset has those dimensions"
    );
    // Each comparison `require` takes.
    for (rule, gaps) in [
        ("require [sub, ses] where bold count<=2", 1),
        ("require [sub, ses] where bold count>1", 1),
        ("require [sub, ses] where bold count!=3", 1),
    ] {
        let settled = settle(&format!("{rule}\n")).unwrap();
        assert_eq!(settled.gaps.len(), gaps, "{rule}: {:?}", settled.gaps);
    }
}

#[test]
fn a_value_clause_on_a_grouping_dimension_explains_the_rule() {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let error = parse_input_spec("drop [sub] where bold missing sub=2\n")
        .unwrap()
        .check(&pipeline)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("`sub` is one of the rule's groups ([sub]), so each group has one `sub`"),
        "{error}"
    );
    assert!(error.contains("write `exclude [sub=2]`"), "{error}");
}

#[test]
fn a_session_missing_its_file_can_be_dropped_rather_than_fail_discovery() {
    let tree = Tree::new(
        "drop-missing",
        &[
            "sub-1/ses-1/run-1.nii",
            "sub-1/ses-1/t1w.nii",
            "sub-1/ses-2/run-1.nii",
        ],
    );
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let discover = "discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}\n";
    let error = parse_input_spec(discover)
        .unwrap()
        .resolve(&pipeline, InputSource::Discover(tree.path()))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("missing source file for `t1w[ses=2,sub=1]`"),
        "{error}"
    );
    let settled = parse_input_spec(&format!("{discover}drop [sub, ses] where t1w count=0\n"))
        .unwrap()
        .resolve(&pipeline, InputSource::Discover(tree.path()))
        .unwrap();
    assert_eq!(
        removed(&settled),
        ["[ses=2,sub=1] by drop [sub, ses] where t1w count=0; found 0"]
    );
}
