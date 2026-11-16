//! Naming sources by the paths they were found at: `*` in `exclude path`,
//! and `prefer path`, which chooses among paths that read as one identity.

mod support;

use support::{spit, text, Tree};

/// Subject 4 exported twice, under two spellings; subject 5 exported once,
/// under the second spelling; subject 6's T1 sent again in a folder of its
/// own, beside its session.
const FOLDERS: [&str; 5] = [
    "export/Proj_A_4_A/Proj A - 1234/anat_T1_9/scan.dcm",
    "export/ProjA_Case_4_A/Proj A - 1234/anat_T1_9/scan.dcm",
    "export/ProjA_Case_5_A/Proj A - 1301/anat_T1_6/scan.dcm",
    "export/Proj_A_6_B/Proj A - 2187/anat_T1_4/scan.dcm",
    "export/Proj_A_6_B/Proj_A_6_B/anat_T1_4/scan.dcm",
];

const PIPELINE: &str = "\
source t1 : Folder/ [id, visit]
operation count(scan: Folder) -> Count .txt
command count: sh -c 'ls \"$1\" > \"$2\"' sh {scan} {@output}
default path: out/{@product}/{id}-{visit}
counts = count(t1)
";

const RECIPE: &str = "\
pipeline study.spit
root export

inputs:
    t1: *_{id:digits}_{visit}/*/anat_T1*
";

/// `spit inputs` on the study with `rules` after its recipe: whether it
/// succeeded, what it printed, and its notes or error.
fn inputs(rules: &str) -> (bool, String, String) {
    let tree = Tree::new("found-paths", &FOLDERS);
    tree.write("study.spit", PIPELINE);
    tree.write("study.spitin", &format!("{RECIPE}\n{rules}"));
    let recipe = tree.path().join("study.spitin");
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    (
        output.status.success(),
        text(&output.stdout),
        text(&output.stderr),
    )
}

#[test]
fn preferences_keep_one_path_of_each_identity_and_record_the_rest() {
    let (ok, printed, notes) =
        inputs("prefer path Proj_A_*    # exported twice\nprefer path */* - *    # sent again\n");
    assert!(ok, "{notes}");
    for record in [
        "t1[id=4,visit=A]: Proj_A_4_A/Proj A - 1234/anat_T1_9",
        "t1[id=5,visit=A]: ProjA_Case_5_A/Proj A - 1301/anat_T1_6",
        "t1[id=6,visit=B]: Proj_A_6_B/Proj A - 2187/anat_T1_4",
    ] {
        assert!(printed.contains(record), "{printed}");
    }
    assert!(
        printed.contains("    t1[id=4,visit=A]\n        rule: prefer path Proj_A_*\n        at: line 7\n        reason: exported twice"),
        "{printed}"
    );
    assert!(
        notes.contains("excluded t1[id=6,visit=B] by `prefer path */* - *` (line 8): sent again"),
        "{notes}"
    );
}

#[test]
fn a_preference_leaves_an_identity_read_once_alone() {
    // `Proj_A_*` covers neither of subject 5's paths, and subject 5 has one,
    // so the rule that would drop it as a copy elsewhere keeps it here.
    let (ok, printed, notes) = inputs("prefer path Proj_A_*\nprefer path */* - *\n");
    assert!(ok, "{notes}");
    assert!(
        printed.contains("t1[id=5,visit=A]: ProjA_Case_5_A/"),
        "{printed}"
    );
}

#[test]
fn a_star_in_an_exclusion_covers_every_copy_it_matches() {
    let (ok, printed, notes) = inputs("exclude path ProjA_Case_*\nprefer path */* - *\n");
    assert!(ok, "{notes}");
    assert!(
        printed.contains("t1[id=4,visit=A]: Proj_A_4_A/"),
        "{printed}"
    );
    // The pattern also covers subject 5's only copy, which the exclusion
    // removes and records.
    assert!(!printed.contains("t1[id=5,visit=A]:"), "{printed}");
    assert!(
        notes.contains("excluded t1[id=5,visit=A] (line 7)"),
        "{notes}"
    );
}

#[test]
fn a_preference_that_leaves_two_paths_is_the_duplicate_error() {
    let (ok, _, error) = inputs("prefer path */* - *\n");
    assert!(!ok);
    assert!(
        error.contains("2 folders read as t1[id=4,visit=A]:\n    ProjA_Case_4_A/Proj A - 1234/anat_T1_9\n    Proj_A_4_A/Proj A - 1234/anat_T1_9\n  no `prefer path` line leaves one path"),
        "{error}"
    );
}

#[test]
fn a_preference_that_covers_no_path_of_a_duplicate_is_the_duplicate_error() {
    let (ok, _, error) = inputs("prefer path Proj_A_*\nprefer path Proj_B_*\n");
    assert!(!ok);
    assert!(
        error.contains("2 folders read as t1[id=6,visit=B]"),
        "{error}"
    );
}

#[test]
fn a_preference_that_chooses_for_no_identity_is_an_error() {
    let (ok, _, error) = inputs(
        "prefer path Proj_A_*\nprefer path */* - *\nprefer path Proj_A_4_A    # never needed\n",
    );
    assert!(!ok);
    assert!(
        error.contains("`prefer path Proj_A_4_A` (line 9) chooses for no identity"),
        "{error}"
    );
}

#[test]
fn found_paths_take_text_and_stars_only() {
    for (rule, message) in [
        ("exclude path sub-{id}", "a path here is text and `*`"),
        ("prefer path */{id}", "a path here is text and `*`"),
        ("prefer */* - *", "`prefer` names the paths to keep"),
        ("prefer path", "`prefer` names the paths to keep"),
    ] {
        let tree = Tree::new("found-paths", &FOLDERS);
        tree.write("study.spit", PIPELINE);
        tree.write("study.spitin", &format!("{RECIPE}\n{rule}\n"));
        let recipe = tree.path().join("study.spitin");
        let output = spit(&["check", recipe.to_str().unwrap()]);
        assert!(!output.status.success(), "{rule}");
        let error = text(&output.stderr);
        assert!(error.contains(message), "{rule}: {error}");
    }
}

#[test]
fn a_preference_belongs_in_the_recipe() {
    let tree = Tree::new("found-paths", &FOLDERS);
    tree.write("study.spit", &format!("{PIPELINE}prefer path */* - *\n"));
    let pipeline = tree.path().join("study.spit");
    let output = spit(&["check", pipeline.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains(".spitin"),
        "{}",
        text(&output.stderr)
    );
}
