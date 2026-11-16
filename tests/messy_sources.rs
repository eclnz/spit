//! Reading sources from folders whose names hold more than the identity:
//! `*` skips text, a rule can fix a dimension's value, a source can have
//! several rules, and records of such a source keep the path discovery found.

mod support;

use std::fs;

use support::{spit, text, Tree};

/// A patient's T1 folder twice, under two spellings, and a control's once.
const FOLDERS: [&str; 3] = [
    "export/Proj_A_4_A/Proj A - 1234/anat_T1_9/scan.dcm",
    "export/ProjA_Case_6_B/ProjA_6/anat_T1_6/scan.dcm",
    "export/Proj_A_Ctrl_6/Proj A - 77/anat_T1_3/scan.dcm",
];

const PIPELINE: &str = "\
source t1 : Folder/ [cohort, id, visit]
operation count(scan: Folder) -> Count .txt
command count: sh -c 'ls \"$1\" > \"$2\"' sh {scan} {@output}
default path: out/{@product}/{cohort}-{id}-{visit}
counts = count(t1)
";

const RECIPE: &str = "\
pipeline study.spit
root export

inputs:
    t1:
        *_{id:digits}_{visit}{cohort=patient}/*/anat_T1*
        *_Ctrl_{id:digits}{cohort=ctrl}{visit=none}/*/anat_T1*
";

fn study(extra: &str) -> Tree {
    let tree = Tree::new("messy", &FOLDERS);
    tree.write("study.spit", PIPELINE);
    tree.write("study.spitin", &format!("{RECIPE}{extra}"));
    tree
}

#[test]
fn a_cohort_split_reads_each_identity_and_keeps_its_folder() {
    let tree = study("");
    let recipe = tree.path().join("study.spitin");
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let printed = text(&output.stdout);
    for record in [
        "t1[cohort=ctrl,id=6,visit=none]: Proj_A_Ctrl_6/Proj A - 77/anat_T1_3",
        "t1[cohort=patient,id=4,visit=A]: Proj_A_4_A/Proj A - 1234/anat_T1_9",
        "t1[cohort=patient,id=6,visit=B]: ProjA_Case_6_B/ProjA_6/anat_T1_6",
    ] {
        assert!(printed.contains(record), "{printed}");
    }
    assert!(
        printed.contains("    t1:\n        *_{id:digits}_{visit}{cohort=patient}/*/anat_T1*\n"),
        "{printed}"
    );
}

#[test]
fn a_spitout_of_read_paths_plans_without_the_recipe() {
    let tree = study("");
    let recipe = tree.path().join("study.spitin");
    let saved = tree.path().join("study.spitout");
    let written = spit(&[
        "inputs",
        recipe.to_str().unwrap(),
        "-o",
        saved.to_str().unwrap(),
    ]);
    assert!(written.status.success(), "{}", text(&written.stderr));
    fs::remove_file(&recipe).unwrap();
    let pipeline = tree.path().join("study.spit");
    let dag = spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        saved.to_str().unwrap(),
        "--paths",
    ]);
    assert!(dag.status.success(), "{}", text(&dag.stderr));
    let paths = text(&dag.stdout);
    assert!(paths.contains("Proj A - 1234/anat_T1_9"), "{paths}");
    assert!(text(&dag.stderr).contains("3 source files verified"));
}

#[test]
fn two_folders_of_one_identity_stop_until_one_is_excluded_by_path() {
    let tree = study("");
    tree.write("export/ProjA_Case_4_A/Proj A - 1234/anat_T1_9/scan.dcm", "");
    let recipe = tree.path().join("study.spitin");
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    let error = text(&output.stderr);
    assert!(
        error.contains(
            "2 folders read as t1[cohort=patient,id=4,visit=A]:\n    ProjA_Case_4_A/Proj A - 1234/anat_T1_9\n    Proj_A_4_A/Proj A - 1234/anat_T1_9\n  keep one with `exclude path <path>` in the recipe"
        ),
        "{error}"
    );

    tree.write(
        "study.spitin",
        &format!("{RECIPE}\nexclude path ProjA_Case_4_A    # sent again\n"),
    );
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let notes = text(&output.stderr);
    assert!(
        notes.contains("excluded t1[cohort=patient,id=4,visit=A] (line 9): sent again"),
        "{notes}"
    );
    let printed = text(&output.stdout);
    assert!(
        printed.contains("rule: exclude path ProjA_Case_4_A"),
        "{printed}"
    );
}

#[test]
fn every_identity_read_twice_is_named_in_one_run() {
    let tree = study("");
    tree.write("export/ProjA_Case_4_A/Proj A - 1234/anat_T1_9/scan.dcm", "");
    tree.write("export/Proj_A_Ctrl_6/Proj_A_6_Ctrl/anat_T1_3/scan.dcm", "");
    let recipe = tree.path().join("study.spitin");
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    let error = text(&output.stderr);
    assert!(
        error.contains(
            "2 folders read as t1[cohort=ctrl,id=6,visit=none]:\n    Proj_A_Ctrl_6/Proj A - 77/anat_T1_3\n    Proj_A_Ctrl_6/Proj_A_6_Ctrl/anat_T1_3\n2 folders read as t1[cohort=patient,id=4,visit=A]:\n    ProjA_Case_4_A/Proj A - 1234/anat_T1_9\n    Proj_A_4_A/Proj A - 1234/anat_T1_9\n  keep one of each with `exclude path <path>` in the recipe"
        ),
        "{error}"
    );
}

#[test]
fn an_exclusion_by_path_that_matches_nothing_fails() {
    let tree = study("\nexclude path Proj_B\n");
    let recipe = tree.path().join("study.spitin");
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("`exclude path Proj_B` (line 9) matches nothing"));
}

#[test]
fn two_rules_of_one_source_may_not_match_one_file() {
    let tree = Tree::new("messy-overlap", &["d/a_1/x.txt"]);
    tree.write(
        "p.spit",
        "source s : T [id]\npath s:\n    *_{id}/x.txt\n    a_{id}/x.txt\n",
    );
    let recipe = tree.write("r.spitin", "pipeline p.spit\nroot d\n");
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains(
            "`a_1/x.txt` matches two path rules of `s`, `*_{id}/x.txt` and `a_{id}/x.txt`"
        ),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_record_of_a_source_that_reads_names_its_file() {
    let tree = Tree::new("messy-record", &["d/a_1/x.txt"]);
    let pipeline = tree.write("p.spit", "source s : T [id]\npath s: *_{id}/x.txt\n");
    let without = tree.write("bare.spitout", "sources:\n    s[id=1]\n");
    let output = spit(&["dag", pipeline.to_str().unwrap(), without.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("each record names its file, as `s[id=1]: <path>`"),
        "{}",
        text(&output.stderr)
    );
    let misread = tree.write("wrong.spitout", "sources:\n    s[id=2]: a_1/x.txt\n");
    let output = spit(&["dag", pipeline.to_str().unwrap(), misread.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr)
            .contains("names `a_1/x.txt`, which its path rule `*_{id}/x.txt` does not read as it"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn rules_that_write_take_no_star_fixed_value_or_second_rule() {
    let tree = Tree::new("messy-writers", &[]);
    for (rule, says) in [
        (
            "default path: out/*/{id}",
            "`*` matches any text, but only a source's path rule reads names",
        ),
        (
            "path o: out/{id}{c=1}",
            "`{c=1}` fixes a value, but only a source's path rule fixes one",
        ),
        (
            "path o:\n    out/a/{id}\n    out/b/{id}",
            "only a source reads its files with several rules",
        ),
    ] {
        let pipeline = tree.write(
            "p.spit",
            &format!(
                "source s : T [id]\npath s: in/{{id}}\noperation f(a: T) -> T\no = f(s)\n{rule}\n"
            ),
        );
        let output = spit(&["check", pipeline.to_str().unwrap()]);
        assert!(!output.status.success(), "{rule}");
        assert!(
            text(&output.stderr).contains(says),
            "{rule}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn a_rule_reads_or_fixes_each_dimension_once_and_only_its_own() {
    let tree = Tree::new("messy-fixed", &[]);
    for (rule, says) in [
        (
            "*{id}/x",
            "has no text between `*` and a value of any length",
        ),
        (
            "*_{id}{id=3}/x",
            "fixes `id`, which `*_{id}{id=3}/x` also names elsewhere",
        ),
        ("*_{id}{c=3}/x", "binds `c`, which `s` does not have"),
        ("{id}_{c=a b}/x", "needs a value with no space"),
        (
            "*_{id}/x\n    *_y/x",
            "omits dimension `id`; each rule of a source binds every dimension",
        ),
    ] {
        let separator = if rule.contains('\n') { ":\n    " } else { ": " };
        let pipeline = tree.write(
            "p.spit",
            &format!("source s : T [id]\npath s{separator}{rule}\n"),
        );
        let output = spit(&["check", pipeline.to_str().unwrap()]);
        assert!(!output.status.success(), "{rule}");
        assert!(
            text(&output.stderr).contains(says),
            "{rule}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn an_editor_is_shown_each_rule_of_a_source() {
    let tree = Tree::new("messy-editor", &[]);
    let pipeline = tree.write(
        "p.spit",
        "source t1 : T .nii [cohort, id]\npath t1:\n    *_{id}{cohort=p}/*\n    *_C_{id}{cohort=c}/*\n",
    );
    let output = spit(&["check", pipeline.to_str().unwrap(), "--json", "--hovers"]);
    let json = text(&output.stdout);
    assert!(
        json.contains(r#"{"product":"t1","line":1,"path":"*_{id}{cohort=p}/*.nii"},{"product":"t1","line":1,"path":"*_C_{id}{cohort=c}/*.nii"}"#),
        "{json}"
    );
    assert!(
        json.contains("Path template: *_{id}{cohort=p}/*.nii or *_C_{id}{cohort=c}/*.nii"),
        "{json}"
    );
}
