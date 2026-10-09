//! File locations can live in a pipeline; selection policy stays in recipes.

mod support;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use support::{text, Tree};

const PIPELINE: &str = "\
root ../data
source raw: Text [id]
path raw: input/{id}.txt
operation copy(raw: Text) -> Text
command copy: cp {raw} {@output}
result = copy(raw)
path result: output/{id}.txt
";

fn run(tree: &Tree, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .current_dir(tree.path())
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

fn dataset(name: &str) -> Tree {
    let tree = Tree::new(name, &["data/input/a.txt", "data/input/b.txt"]);
    tree.write("code/analysis.spit", PIPELINE);
    tree
}

#[test]
fn one_file_checks_scans_and_resolves_from_another_working_folder() {
    let tree = dataset("inline-workflow");
    success(run(&tree, &["check", "code/analysis.spit"]));
    let records = success(run(&tree, &["inputs", "code/analysis.spit"]));
    assert!(records.contains("raw[id=a]"), "{records}");
    assert!(records.contains("raw[id=b]"), "{records}");
    let commands = success(run(&tree, &["dag", "code/analysis.spit"]));
    assert!(
        commands.contains("cp input/a.txt output/a.txt"),
        "{commands}"
    );
    assert!(
        commands.contains("cp input/b.txt output/b.txt"),
        "{commands}"
    );
    success(run(&tree, &["artifacts", "code/analysis.spit"]));
    // Saving the settled inventory retains the same source files and jobs.
    std::fs::create_dir_all(tree.path().join("saved")).unwrap();
    success(run(
        &tree,
        &["inputs", "code/analysis.spit", "-o", "saved/inputs.spitout"],
    ));
    let written = std::fs::read_to_string(tree.path().join("saved/inputs.spitout")).unwrap();
    assert!(written.starts_with("root ../data\n"), "{written}");
    let saved = success(run(
        &tree,
        &["dag", "code/analysis.spit", "saved/inputs.spitout"],
    ));
    assert_eq!(commands, saved);
}

#[test]
fn recipes_inherit_the_pipeline_root_and_add_selection_rules() {
    let tree = dataset("inline-selection");
    // The recipe's folder differs from the pipeline's: the inherited root
    // still belongs to the declaring pipeline, not to this recipe.
    tree.write(
        "recipes/selected.spitin",
        "\
pipeline ../code/analysis.spit
discover ids: [id] from dirs groups/{id}
require [id] where raw count=1
exclude [id=b]
",
    );
    tree.write("data/groups/a/marker", "");
    tree.write("data/groups/b/marker", "");
    success(run(&tree, &["check", "recipes/selected.spitin"]));
    let records = success(run(&tree, &["inputs", "recipes/selected.spitin"]));
    let inventory = spit::parse_source_inventory(&records).unwrap();
    assert_eq!(inventory.artifacts.len(), 1);
    assert_eq!(inventory.artifacts[0].entities.get("id"), Some("a"));
    let commands = success(run(&tree, &["dag", "recipes/selected.spitin"]));
    assert!(
        commands.contains("cp input/a.txt output/a.txt"),
        "{commands}"
    );
    assert!(!commands.contains("cp input/b.txt"), "{commands}");
}

#[test]
fn duplicate_roots_and_source_paths_are_errors_even_when_equal() {
    let tree = dataset("inline-conflicts");
    for root in ["../data", "elsewhere"] {
        tree.write(
            "code/analysis.spitin",
            &format!("pipeline analysis.spit\nroot {root}\n"),
        );
        for command in ["check", "inputs", "dag", "artifacts"] {
            let output = run(&tree, &[command, "code/analysis.spitin"]);
            assert!(!output.status.success(), "{command}");
            assert!(
                text(&output.stderr).contains("root is declared in both"),
                "{}",
                text(&output.stderr)
            );
        }
        let output = run(&tree, &["check", "code/analysis.spitin", "--json"]);
        let json = text(&output.stdout);
        assert!(!output.status.success());
        assert!(json.contains("\"line\":2"), "{json}");
        assert!(json.contains("root is declared in both"), "{json}");
    }
    tree.write(
        "code/analysis.spitin",
        "pipeline analysis.spit\npath raw: input/{id}.txt\n",
    );
    for command in ["check", "inputs", "dag"] {
        let output = run(&tree, &[command, "code/analysis.spitin"]);
        assert!(!output.status.success());
        assert!(
            text(&output.stderr).contains("path rules in both"),
            "{}",
            text(&output.stderr)
        );
    }
    let output = run(&tree, &["inputs", "code/analysis.spit", "--root", "data"]);
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("both .spit and `--root`"));
}

#[test]
fn every_selection_form_is_recipe_only_including_literal_exclusions() {
    let tree = dataset("inline-boundary");
    for rule in [
        "discover ids: [id] from dirs groups/{id}",
        "require [id] where raw count=1",
        "exclude raw[id=b]",
        "exclude [id=b]",
        "exclude raw",
        "exclude from excluded.csv",
        "exclude [id] where raw count<1",
    ] {
        tree.write("code/rejected.spit", &format!("{PIPELINE}{rule}\n"));
        let output = run(
            &tree,
            &["check", "code/rejected.spit", "--json", "--hovers"],
        );
        assert!(!output.status.success(), "{rule}");
        let json = text(&output.stdout);
        assert!(
            json.contains("belong in a .spitin recipe"),
            "{rule}: {json}"
        );
        assert!(json.contains("\"line\":8"), "{rule}: {json}");
    }
}

#[test]
fn roots_are_top_level_once_and_require_a_directory() {
    for (text, message) in [
        ("root a\nroot b\n", "names its root once"),
        ("root\n", "expected `root <directory>`"),
        ("root  # empty\n", "expected `root <directory>`"),
        (
            "stage processing:\n  root data\n",
            "belongs at the top level",
        ),
        ("operation copy(raw: Text) -> Text:\n  root data\n", "body"),
    ] {
        let error = spit::parse_pipeline(text).unwrap_err();
        assert!(error.to_string().contains(message), "{text}: {error}");
    }
    let pipeline = spit::parse_pipeline("root data=set\nsource raw: Text\noperation copy(raw: Text) -> Text\nroot : Text = copy(raw)\n").unwrap();
    assert_eq!(
        pipeline.root.as_ref().map(|(root, _)| root.as_path()),
        Some(Path::new("data=set"))
    );
    assert!(pipeline
        .products
        .iter()
        .any(|product| product.name == "root"));
}

#[test]
fn imported_definitions_do_not_import_a_dataset_root() {
    let tree = dataset("inline-import");
    tree.write(
        "library.spit",
        "root nowhere\noperation copy(raw: Text) -> Text\n",
    );
    tree.write(
        "code/analysis.spit",
        &PIPELINE.replace("operation copy(raw: Text) -> Text", "use ../library.spit"),
    );
    success(run(&tree, &["dag", "code/analysis.spit"]));
    tree.write("code/unbound.spit", "use ../library.spit\n");
    let parsed = spit::parse_pipeline_at(
        "use ../library.spit\n",
        &tree.path().join("code/unbound.spit"),
    )
    .unwrap();
    assert!(parsed.root.is_none());
}

#[test]
fn stdin_checks_resolve_and_locate_the_inline_root() {
    let tree = dataset("inline-stdin");
    let path = tree.path().join("code/analysis.spit");
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            path.to_str().unwrap(),
            "--stdin",
            "--json",
            "--hovers",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PIPELINE.replace("../data", "../missing").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let json = success(output);
    assert!(json.contains("\"line\":1"), "{json}");
    assert!(
        json.contains("dataset root `../missing` is not a folder"),
        "{json}"
    );
    assert!(json.contains("inherits its pipeline's root"), "{json}");
}

#[test]
fn absolute_roots_and_spaces_are_directory_text() {
    let tree = Tree::new("inline-absolute", &["data with spaces/input/a.txt"]);
    let root = tree.path().join("data with spaces");
    tree.write(
        "code/analysis.spit",
        &PIPELINE.replace("../data", root.to_str().unwrap()),
    );
    let commands = success(run(&tree, &["dag", "code/analysis.spit"]));
    assert!(
        commands.contains("cp input/a.txt output/a.txt"),
        "{commands}"
    );
}

#[test]
fn an_inherited_missing_root_warns_at_the_pipeline_declaration() {
    let tree = dataset("inline-inherited-warning");
    tree.write(
        "code/analysis.spit",
        &PIPELINE.replace("../data", "../missing"),
    );
    let path = tree.write(
        "recipes/selected.spitin",
        "pipeline ../code/analysis.spit\n",
    );
    let diagnostics = spit::diagnose_recipe("pipeline ../code/analysis.spit\n", &path);
    let warning = diagnostics
        .iter()
        .find(|item| item.message.starts_with("dataset root "))
        .unwrap();
    assert_eq!(warning.file.as_deref(), Some("../code/analysis.spit"));
    assert_eq!(warning.line, Some(1));
    assert!(!diagnostics.iter().any(spit::Diagnostic::is_error));
}
