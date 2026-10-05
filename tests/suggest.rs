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
# sub: 01, 02; ses: 1; run: 1, 2
# for `bold`, which the pipeline declares
path bold: sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz

# 4 files, such as sub-01/ses-1/anat/sub-01_ses-1_T1w.json
# sub: 01, 02; ses: 1
source t1w_json .json [sub, ses]
source t1w_nii_gz .nii.gz beside t1w_json
path t1w_json: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w.json

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
             #   source t1w_json .json [sub, ses]\n\
             #   source t1w_nii_gz .nii.gz beside t1w_json\n\
             path t1w_json: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w.json\n"
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
        out.contains("path baseline: baseline/{dim1}/{date:date}.csv"),
        "{out}"
    );
    assert!(
        out.contains("path raw: raw/{dim1}/{date:date}.csv"),
        "{out}"
    );
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

#[test]
fn files_in_a_source_folder_are_read_with_it() {
    let tree = Tree::new(
        "suggest-folder",
        &[
            "dicom/sub-01/0001.dcm",
            "dicom/sub-01/0002.dcm",
            "dicom/sub-02/0001.dcm",
            "notes/sub-01.txt",
            "notes/sub-02.txt",
        ],
    );
    tree.write(
        "a.spit",
        "source dicom : Dicom / [sub]\npath dicom: dicom/sub-{sub}\n",
    );
    let out = suggested(&tree, &["inputs", "a.spit", "--root", ".", "--suggest"]);
    assert!(!out.contains(".dcm"), "{out}");
    assert!(out.contains("path notes: notes/sub-{sub}.txt"), "{out}");
}

/// A lab's own layout, with the slips a real one has: a folder in the wrong
/// case, a backup copy, a repeat scan, a copy with a space in its name.
const LAB: [&str; 9] = [
    "Subject01/Visit1/T1_2024-01-15.nii",
    "Subject01/Visit2/T1_2024-03-02.nii",
    "Subject02/Visit1/T1_2024-01-20.nii",
    "Subject02/Visit2/T1_2024-03-09.nii",
    "Subject10/Visit1/T1_2024-02-11.nii",
    "Subject10/Visit1/T1_2024-02-11_repeat.nii",
    "Subject01/Visit1/T1_2024-01-15.nii.bak",
    "subject04/visit1/T1_2024-04-01.nii",
    "README",
];

#[test]
fn a_stray_folder_loses_only_its_own_files() {
    let tree = Tree::new("suggest-stray", &LAB);
    tree.write("empty.spit", "");
    let out = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    // `subject04` alone would otherwise turn every word into `dim1`, `dim2`.
    assert!(
        out.contains("source t1 [subject, visit, date]\npath t1: Subject{subject}/Visit{visit}/T1_{date:date}.nii\n"),
        "{out}"
    );
    assert!(
        out.contains("# subject: 01, 02, 10; visit: 1, 2; date: 2024-01-15, 2024-01-20, 2024-02-11, 2024-03-02, 2024-03-09\n"),
        "{out}"
    );
    assert!(
        out.contains("#   subject04/visit1/T1_2024-04-01.nii\n"),
        "{out}"
    );
}

#[test]
fn files_a_rule_nearly_matches_say_where_they_part_from_it() {
    let tree = Tree::new("suggest-near", &LAB);
    tree.write("empty.spit", "");
    let out = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    assert!(
        out.contains("# 2 files a rule above nearly matches but will not read; rename them, or give them a rule of their own:\n"),
        "{out}"
    );
    assert!(
        out.contains("#   Subject01/Visit1/T1_2024-01-15.nii.bak\n#     `t1`: after `Subject01/Visit1/T1_2024-01-15.nii`, the file has `.bak` where the rule ends\n"),
        "{out}"
    );
    assert!(
        out.contains("#     `t1`: after `Subject10/Visit1/T1_2024-02-11`, the file has `_repeat.nii` where the rule has `.nii`\n"),
        "{out}"
    );
}

#[test]
fn a_rule_of_dimensions_alone_is_not_suggested() {
    let tree = Tree::new("suggest-bare", &["README", "CHANGES", "LICENSE"]);
    tree.write("empty.spit", "");
    let out = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    // `path files: {dim1}` would read every file and folder at the root.
    assert!(!out.contains("path"), "{out}");
    assert!(out.contains("# 3 files like no other"), "{out}");
}

#[test]
fn dates_years_and_a_word_before_name_dimensions() {
    let tree = Tree::new(
        "suggest-names",
        &[
            "site_north/2023/logger7/2023-03-01.csv",
            "site_north/2023/logger7/2023-03-02.csv",
            "site_south/2024/logger12/2024-01-05.csv",
            "plate1/field_001.tif",
            "plate1/field_002.tif",
        ],
    );
    tree.write("empty.spit", "");
    let out = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    // `site_north` and `site_south` are one folder, and the source is not
    // named for its `site` dimension.
    assert!(
        out.contains("source csv [site, year, logger, date]\npath csv: site_{site}/{year:year}/logger{logger}/{date:date}.csv\n"),
        "{out}"
    );
    assert!(
        out.contains("path tif: plate{plate}/field_{field}.tif"),
        "{out}"
    );
    assert!(!out.contains("named by place"), "{out}");
}

#[test]
fn groups_of_one_suffix_are_named_by_their_own_word() {
    let tree = Tree::new(
        "suggest-own-word",
        &[
            "sub-01/func/sub-01_task-rest_run-1_bold.nii.gz",
            "sub-02/func/sub-02_task-rest_run-1_bold.nii.gz",
            "sub-03/func/sub-03_task-rest_run-1_bold.nii.gz",
            "sub-01/func/sub-01_task-nback_bold.nii.gz",
            "sub-02/func/sub-02_task-nback_bold.nii.gz",
        ],
    );
    tree.write("empty.spit", "");
    let out = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    // One task has runs and the other none, so they are two sources.
    assert!(out.contains("source bold [sub, run]\n"), "{out}");
    assert!(out.contains("source bold_nback [sub]\n"), "{out}");
}

#[test]
fn pasted_suggestions_for_an_irregular_dataset_read_their_files() {
    let tree = Tree::new("suggest-irregular-paste", &LAB);
    tree.write("empty.spit", "");
    let lines = suggested(&tree, &["inputs", "empty.spit", "--root", ".", "--suggest"]);
    tree.write(
        "b.spit",
        &format!("{lines}\noperation view(t1) -> .txt\ncommand view: cp {{t1}} {{@output}}\nseen = view(t1)\n"),
    );
    let output = spit(&tree, &["inputs", "b.spit", "--root", "."]);
    let stderr = text(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("note: found 5 source artifacts"),
        "{stderr}"
    );
}

#[test]
fn files_at_the_pipelines_output_paths_are_not_suggested_as_sources() {
    let tree = Tree::new(
        "suggest-outputs",
        &[
            "data/sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz",
            "data/sub-02/ses-1/func/sub-02_ses-1_task-rest_run-1_bold.nii.gz",
            "data/out/moco/sub=01__ses=1__run=1.nii.gz",
            "data/out/moco/sub=02__ses=1__run=1.nii.gz",
        ],
    );
    tree.write("a.spit", &format!("source bold [sub, ses, run]\n{STEPS}"));
    let expected = "\
# 2 files, such as sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz
# sub: 01, 02; ses: 1; run: 1
# for `bold`, which the pipeline declares
path bold: sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz
";
    assert_eq!(
        suggested(&tree, &["inputs", "a.spit", "--root", "data", "--suggest"]),
        expected
    );
}
