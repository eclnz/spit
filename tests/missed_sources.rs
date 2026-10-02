//! A source whose path rule matches no file under the scanned root is
//! named, with the unmatched file nearest the rule and where the two part,
//! whether the rule is in the pipeline, read with `--root`, or in a recipe.

mod support;

use std::process::Command;

use support::{text, Tree};

const BOLD: [&str; 3] = [
    "data/sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz",
    "data/sub-01/ses-1/anat/sub-01_ses-1_T1w.nii.gz",
    "data/participants.tsv",
];

const STEPS: &str = "\
operation mc(bold) -> .nii.gz
command mc: mcflirt -in {bold} -out {@output}
moco = mc(bold)
";

fn stderr(tree: &Tree, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .current_dir(tree.path())
        .output()
        .unwrap();
    text(&output.stderr)
}

#[test]
fn a_pipeline_rule_names_the_nearest_file_and_where_it_parts() {
    let tree = Tree::new("missed-pipeline", &BOLD);
    tree.write(
        "a.spit",
        &format!(
            "source bold [sub, ses, run]\n\
             path bold: data/sub-{{sub}}/ses-{{ses}}/func/sub-{{sub}}_ses-{{ses}}_run-{{run}}_bold.nii.gz\n{STEPS}"
        ),
    );
    let stderr = stderr(&tree, &["dag", "a.spit", "--root", "."]);
    let expected = "\
warning: source `bold` matched no files with path rule `data/sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_run-{run}_bold.nii.gz`
  the nearest file is `data/sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz`
  after `data/sub-01/ses-1/func/sub-01_ses-1_`, the file has `task-rest_run-1_bold.nii.gz` where the rule has `run-{run}_bold.nii.gz`
";
    assert!(stderr.starts_with(expected), "{stderr}");
}

#[test]
fn a_recipe_rule_names_the_nearest_file() {
    let tree = Tree::new("missed-recipe", &BOLD);
    tree.write("a.spit", &format!("source bold [sub, ses, run]\n{STEPS}"));
    tree.write(
        "a.spitin",
        "pipeline a.spit\nroot data\npath bold: sub-{sub}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz\n",
    );
    let stderr = stderr(&tree, &["inputs", "a.spitin"]);
    assert!(
        stderr.contains(
            "  after `sub-01/`, the file has `ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz` \
             where the rule has `func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz`"
        ),
        "{stderr}"
    );
}

#[test]
fn a_file_that_goes_on_past_the_rule_says_where_the_rule_ends() {
    let tree = Tree::new("missed-ends", &["scans/a/1.txt.bak"]);
    tree.write(
        "a.spit",
        "source scan [id, run]\npath scan: scans/{id}/{run}.txt\n\
         operation copy(scan) -> .txt\ncommand copy: cp {scan} {@output}\ncopied = copy(scan)\n",
    );
    let stderr = stderr(&tree, &["inputs", "a.spit", "--root", "."]);
    assert!(
        stderr.contains("  after `scans/a/1.txt`, the file has `.bak` where the rule ends"),
        "{stderr}"
    );
}

#[test]
fn a_rule_no_file_comes_near_names_no_file() {
    let tree = Tree::new("missed-far", &BOLD);
    tree.write(
        "a.spit",
        &format!(
            "source bold [sub, ses, run]\npath bold: raw/{{sub}}/{{ses}}/{{run}}.mgz\n{STEPS}"
        ),
    );
    let stderr = stderr(&tree, &["inputs", "a.spit", "--root", "."]);
    assert!(
        stderr.starts_with(
            "warning: source `bold` matched no files with path rule `raw/{sub}/{ses}/{run}.mgz`\nnote: "
        ),
        "{stderr}"
    );
}

#[test]
fn a_coverage_failure_points_to_the_nearest_file() {
    let tree = Tree::new("missed-coverage", &BOLD);
    // The T1w images give the groups the `require` rule checks.
    tree.write(
        "a.spit",
        &format!("source bold [sub, ses, run]\nsource t1w [sub, ses]\n{STEPS}"),
    );
    tree.write(
        "a.spitin",
        "pipeline a.spit\nroot data\n\
         path bold: sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_run-{run}_bold.nii.gz\n\
         path t1w: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w.nii.gz\n\
         require bold count>=1 per [sub, ses]\n",
    );
    let stderr = stderr(&tree, &["inputs", "a.spitin"]);
    assert!(
        stderr.contains("  the nearest file is `sub-01/ses-1/func/"),
        "{stderr}"
    );
    assert!(
        stderr.contains("the warning above names the nearest file"),
        "{stderr}"
    );
}
