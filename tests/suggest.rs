//! `spit inputs --suggest` groups the files no source rule matches by shape
//! and prints a source and path rule for each group, for a pipeline given
//! with `--root` or for a recipe, and every rule it prints matches its
//! files when pasted in.

mod support;

use std::process::{Command, Output};

use support::{text, Tree};

const COHORT: [&str; 9] = [
    "data/sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz",
    "data/sub-01/ses-1/func/sub-01_ses-1_task-rest_run-2_bold.nii.gz",
    "data/sub-02/ses-1/func/sub-02_ses-1_task-rest_run-1_bold.nii.gz",
    "data/sub-01/ses-1/anat/sub-01_ses-1_T1w.nii.gz",
    "data/sub-01/ses-1/anat/sub-01_ses-1_T1w.json",
    "data/sub-02/ses-1/anat/sub-02_ses-1_T1w.nii.gz",
    "data/sub-02/ses-1/anat/sub-02_ses-1_T1w.json",
    "data/participants.tsv",
    "data/cohort.spitin",
];

const STEPS: &str = "\
operation mc(bold) -> .nii.gz
command mc: mcflirt -in {bold} -out {@output}
moco = mc(bold)
";

fn spit(tree: &Tree, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .current_dir(tree.path())
        .output()
        .unwrap()
}

fn suggested(tree: &Tree, args: &[&str]) -> String {
    let output = spit(tree, args);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

#[test]
fn a_pipeline_gets_sources_and_rules_and_a_declared_source_its_rule() {
    let tree = Tree::new("suggest-pipeline", &COHORT);
    tree.write(
        "a.spit",
        &format!("source bold [sub, ses, run]\nsource fmap [sub]\n{STEPS}"),
    );
    let expected = "\
# 3 files, such as sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz
# for `bold`, which the pipeline declares
path bold: sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz

# 4 files, such as sub-01/ses-1/anat/sub-01_ses-1_T1w.json
sidecars t1w [sub, ses]:
    path: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w
    source t1w_json .json
    source t1w_nii_gz .nii.gz

# 1 file like no other, each a source with no dimensions if a step reads it:
#   participants.tsv

# no group of files above fits `fmap` alone; give it a path rule, or rename a source above to it
";
    assert_eq!(
        suggested(&tree, &["inputs", "a.spit", "--root", "data", "--suggest"]),
        expected
    );
}

#[test]
fn a_recipe_gets_its_path_rules_and_its_pipeline_the_source_lines() {
    let tree = Tree::new("suggest-recipe", &COHORT);
    tree.write("a.spit", &format!("source bold [sub, ses, run]\n{STEPS}"));
    tree.write("a.spitin", "pipeline a.spit\nroot data\n");
    let out = suggested(&tree, &["inputs", "a.spitin", "--suggest"]);
    assert!(
        out.contains(
            "# in a.spit:\n\
             #   sidecars t1w [sub, ses]:\n\
             #       source t1w_json .json\n\
             #       source t1w_nii_gz .nii.gz\n\
             path t1w: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w\n"
        ),
        "{out}"
    );
    assert!(
        out.contains("path bold: sub-{sub}/ses-{ses}/func/"),
        "{out}"
    );
}

#[test]
fn pasted_suggestions_find_every_file_they_were_made_from() {
    let tree = Tree::new("suggest-paste", &COHORT);
    tree.write("empty.spit", "");
    let lines = suggested(
        &tree,
        &["inputs", "empty.spit", "--root", "data", "--suggest"],
    );
    tree.write(
        "b.spit",
        &format!("{lines}\noperation mc(bold) -> .nii.gz\ncommand mc: mcflirt {{bold}} {{@output}}\nmoco = mc(bold)\n"),
    );
    let output = spit(&tree, &["inputs", "b.spit", "--root", "data"]);
    let stderr = text(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("note: found 7 source artifacts"),
        "{stderr}"
    );
}

#[test]
fn folders_and_names_of_different_forms_are_each_one_value() {
    let tree = Tree::new(
        "suggest-forms",
        &[
            "sweep/lr-high/seed-11.json",
            "sweep/lr-low/seed-11.json",
            "sweep/warmup/seed-22.json",
            "stores/s01/rev2.json",
            "stores/S07/rev3.json",
        ],
    );
    tree.write("empty.spit", "");
    let out = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    assert!(
        out.contains("path sweep: sweep/{dim1}/seed-{seed}.json"),
        "{out}"
    );
    // `s01` is a value, while `rev2` names its digits.
    assert!(out.contains("source stores [dim1, rev]\n"), "{out}");
    assert!(
        out.contains("# dim1 named by place, as no word in the path names them"),
        "{out}"
    );
}

#[test]
fn a_plain_top_folder_keeps_its_files_apart() {
    let tree = Tree::new(
        "suggest-top",
        &[
            "baseline/east/2025-12-30.csv",
            "baseline/north/2025-11-14.csv",
            "raw/east/2026-06-01.csv",
            "raw/north/2026-06-02.csv",
        ],
    );
    tree.write("empty.spit", "");
    let out = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    assert!(
        out.contains("path baseline: baseline/{dim1}/{dim2}.csv"),
        "{out}"
    );
    assert!(out.contains("path raw: raw/{dim1}/{dim2}.csv"), "{out}");
}

#[test]
fn nothing_to_suggest_when_every_file_matches() {
    let tree = Tree::new("suggest-none", &["in/a.txt", "in/b.txt"]);
    tree.write("a.spit", "source item [id]\npath item: in/{id}.txt\n");
    let output = spit(&tree, &["inputs", "a.spit", "--root", ".", "--suggest"]);
    assert!(output.status.success());
    assert_eq!(text(&output.stdout), "");
    assert!(
        text(&output.stderr).contains("note: every file under `.` matches a source rule"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn suggest_writes_no_spitout() {
    let tree = Tree::new("suggest-output", &["in/a.txt"]);
    tree.write("a.spit", "");
    let output = spit(
        &tree,
        &[
            "inputs",
            "a.spit",
            "--root",
            ".",
            "--suggest",
            "-o",
            "x.spitout",
        ],
    );
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("--suggest"),
        "{}",
        text(&output.stderr)
    );
}
