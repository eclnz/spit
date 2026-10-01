//! A recipe's `root` line names where its dataset is, so `inputs` and `dag`
//! need no `--root`, and a `.spitout` records the root it was settled
//! against, so `dag` on it needs none either.

mod support;

use std::path::Path;
use std::process::{Command, Output};

use support::{text, Tree};

const PIPELINE: &str = "\
source image: Image [sub, ses]
operation process(image: Image) -> Image
command process: tool {image} {output}
result = process(image)
path result: results/{sub}_{ses}.nii.gz
";

/// Rules relative to the dataset folder, not the recipe's.
const RULES: &str = "\
discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}
path image: sub-{sub}/ses-{ses}/image.nii.gz
";

const FILES: [&str; 3] = [
    "data/sub-1/ses-1/image.nii.gz",
    "data/sub-1/ses-2/image.nii.gz",
    "data/sub-5/ses-1/image.nii.gz",
];

/// `spit` run from the folder `cwd`.
fn spit_in(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

/// A dataset under `data/`, with the pipeline and a recipe whose header is
/// `header` beside it.
fn dataset(name: &str, header: &str) -> Tree {
    let tree = Tree::new(name, &FILES);
    tree.write("analysis.spit", PIPELINE);
    tree.write(
        "analysis.spitin",
        &format!("pipeline analysis.spit\n{header}{RULES}"),
    );
    tree
}

fn succeeded(output: &Output) -> (String, String) {
    let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
    assert!(output.status.success(), "{stderr}");
    (stdout, stderr)
}

#[test]
fn a_root_line_stands_in_for_the_flag() {
    let tree = dataset("root-line", "root data\n");
    let named = spit_in(tree.path(), &["dag", "analysis.spitin", "--commands"]);
    let (named, notes) = succeeded(&named);
    assert!(notes.contains("note: 3 source files verified."), "{notes}");
    assert!(
        named.contains("tool sub-5/ses-1/image.nii.gz results/5_1.nii.gz"),
        "{named}"
    );
    let flagged = spit_in(
        tree.path(),
        &["dag", "analysis.spitin", "--root", "data", "--commands"],
    );
    assert_eq!(named, succeeded(&flagged).0);
    let found = spit_in(tree.path(), &["inputs", "analysis.spitin"]);
    let (_, notes) = succeeded(&found);
    assert!(
        notes.contains("note: found 3 source artifacts and 3 contexts"),
        "{notes}"
    );
}

#[test]
fn the_flag_overrides_the_root_line() {
    let tree = dataset("root-override", "root elsewhere\n");
    let output = spit_in(tree.path(), &["dag", "analysis.spitin", "--root", "data"]);
    let (_, notes) = succeeded(&output);
    assert!(notes.contains("note: 3 source files verified."), "{notes}");
}

#[test]
fn the_root_line_is_relative_to_the_recipe_or_absolute() {
    let tree = Tree::new("root-relative", &FILES);
    tree.write("code/analysis.spit", PIPELINE);
    tree.write(
        "code/analysis.spitin",
        &format!("pipeline analysis.spit\nroot ../data\n{RULES}"),
    );
    // Run from elsewhere: the line is read from the recipe's folder.
    let output = spit_in(tree.path(), &["dag", "code/analysis.spitin"]);
    let (_, notes) = succeeded(&output);
    assert!(notes.contains("note: 3 source files verified."), "{notes}");

    let absolute = tree.path().join("data");
    tree.write(
        "code/absolute.spitin",
        &format!(
            "pipeline analysis.spit\nroot {}\n{RULES}",
            absolute.display()
        ),
    );
    let output = spit_in(tree.path(), &["dag", "code/absolute.spitin"]);
    let (_, notes) = succeeded(&output);
    assert!(notes.contains("note: 3 source files verified."), "{notes}");
}

#[test]
fn a_root_line_does_not_make_written_records_a_scan() {
    let tree = Tree::new("root-records", &FILES);
    tree.write("analysis.spit", PIPELINE);
    tree.write(
        "analysis.spitin",
        "pipeline analysis.spit\nroot data\npath image: sub-{sub}/ses-{ses}/image.nii.gz\nsources:\n    image[sub=1,ses=1]\n",
    );
    let output = spit_in(tree.path(), &["inputs", "analysis.spitin"]);
    let (records, _) = succeeded(&output);
    assert!(records.contains("image[sub=1,ses=1]"), "{records}");
    assert!(!records.contains("sub=5"), "{records}");
    // The records' files are still checked under the root.
    let output = spit_in(tree.path(), &["dag", "analysis.spitin"]);
    let (_, notes) = succeeded(&output);
    assert!(notes.contains("note: 1 source files verified."), "{notes}");
}

#[test]
fn a_spitout_records_its_root_relative_to_itself() {
    let tree = dataset("root-spitout", "root data\n");
    std::fs::create_dir(tree.path().join("plans")).unwrap();
    let output = spit_in(
        tree.path(),
        &["inputs", "analysis.spitin", "-o", "plans/analysis.spitout"],
    );
    succeeded(&output);
    let written = std::fs::read_to_string(tree.path().join("plans/analysis.spitout")).unwrap();
    assert!(written.starts_with("root ../data\n\n"), "{written}");
    // From another folder, `dag` reads the root from the .spitout.
    let output = spit_in(
        &tree.path().join("plans"),
        &["dag", "../analysis.spit", "analysis.spitout", "--commands"],
    );
    let (commands, notes) = succeeded(&output);
    assert!(notes.contains("note: 3 source files verified."), "{notes}");
    assert!(
        notes.contains("note: commands run from `../data`"),
        "{notes}"
    );
    assert!(
        commands.contains("tool sub-1/ses-2/image.nii.gz"),
        "{commands}"
    );
    // `--root` overrides it.
    let output = spit_in(
        &tree.path().join("plans"),
        &[
            "dag",
            "../analysis.spit",
            "analysis.spitout",
            "--root",
            "../plans",
        ],
    );
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("sub-1/ses-1/image.nii.gz"));
}

#[test]
fn a_spitout_names_its_root_once_and_first() {
    let twice = spit::parse_source_inventory("root a\nroot b\nsources:\n    x\n").unwrap_err();
    assert!(twice.to_string().contains("names its root once"), "{twice}");
    let late = spit::parse_source_inventory("sources:\n    x\nroot a\n").unwrap_err();
    assert!(late.to_string().contains("before every section"), "{late}");
    let read = spit::parse_source_inventory("root ../data\n\nsources:\n    x\n").unwrap();
    assert_eq!(read.root.as_deref(), Some(Path::new("../data")));
}

#[test]
fn a_recipe_names_its_root_once() {
    let tree = dataset("root-twice", "root data\nroot other\n");
    let output = spit_in(tree.path(), &["check", "analysis.spitin"]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("a .spitin names its root once"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn check_warns_when_the_root_is_missing() {
    let tree = dataset("root-missing", "root nowhere  # not made yet\n");
    let output = spit_in(tree.path(), &["check", "analysis.spitin"]);
    let (stdout, stderr) = succeeded(&output);
    assert!(stdout.contains("Recipe valid."), "{stdout}");
    assert!(
        stderr.contains("line 2, column 6: dataset root `nowhere` is not a folder"),
        "{stderr}"
    );
    let output = spit_in(tree.path(), &["check", "analysis.spitin", "--json"]);
    let (json, _) = succeeded(&output);
    assert!(json.contains("\"severity\":\"warning\""), "{json}");
    assert!(json.contains("\"line\":2"), "{json}");
    // A root that is there draws no warning.
    let tree = dataset("root-present", "root data\n");
    let output = spit_in(tree.path(), &["check", "analysis.spitin"]);
    let (_, stderr) = succeeded(&output);
    assert!(!stderr.contains("warning"), "{stderr}");
}

#[test]
fn a_recipe_inside_its_root_is_not_an_unmatched_file() {
    let tree = Tree::new("root-inside", &FILES);
    tree.write("data/analysis.spit", PIPELINE);
    tree.write(
        "data/analysis.spitin",
        &format!("pipeline analysis.spit\n{RULES}"),
    );
    tree.write("data/notes.txt", "");
    let output = spit_in(tree.path(), &["inputs", "data/analysis.spitin"]);
    let (_, notes) = succeeded(&output);
    assert!(notes.contains("note: 1 files under"), "{notes}");
    let output = spit_in(
        tree.path(),
        &["inputs", "data/analysis.spitin", "--unmatched"],
    );
    assert_eq!(succeeded(&output).0, "notes.txt\n");
}
