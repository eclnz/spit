//! What the CLI prints and writes over small generated datasets, compared
//! byte for byte with `tests/fixtures/outputs/`, so that a change to any
//! output fails here. When a change is meant, run with `SPIT_BLESS=1` to
//! write the new outputs, and review their diff.

mod support;

use std::path::Path;
use std::process::Command;

use spit::{
    diagnose_checked_with_inventory, diagnose_checked_with_records, parse_input_spec,
    parse_pipeline, render_artifacts, render_dag, render_source_inventory, Context, InputSource,
};
use support::Tree;

const PIPELINE: &str = "\
path: derivatives/{@product}/{@entities}.txt
source image : Image<T1w> [sub, ses, run]
path image: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_run-{run}_T1w.nii
source mask : Mask [sub]
path mask: sub-{sub}/mask.nii
source reference : Reference [sub, ses]
path reference: sub-{sub}/ses-{ses}/reference.nii

stage prep:
    operation clean(image: Image<$K>, mask: Mask) -> Image<$K>
    command clean: denoise {image} --mask {mask} -o {@output}
    cleaned = clean(image, mask)

    operation align(image: Image<$K>, reference: Reference) -> Image<$K>
    command align: register {image} {reference} {@output}
    aligned = align(cleaned, reference)

operation average(images: many Image<$K>) -> Image<$K>
command average: mean {images} -o {@output}
averaged = average(aligned @ vary(run))

operation compare(image: Image<$K>, reference: Reference) -> Score
command compare: score {image} {reference} -o {@output}
score = compare(averaged, reference)
";

const DISCOVER: &str = "discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}\n";
/// A `require` rule that the gaps dataset fails.
const STRICT: &str = "\
require [sub, ses] where image count>=2
require [sub, ses] where reference count=1
";
/// A `drop` rule that removes what the gaps dataset lacks.
const DROP: &str = "\
drop [sub, ses] where image count<2
require [sub, ses] where reference count=1
";

/// Three subjects with a mask, each with two sessions of a reference and
/// two runs; `gaps` leaves out a run of `sub-02`'s second session.
fn dataset(gaps: bool) -> Tree {
    let mut files = Vec::new();
    for sub in 1..=3 {
        files.push(format!("sub-{sub:02}/mask.nii"));
        for ses in 1..=2 {
            files.push(format!("sub-{sub:02}/ses-{ses}/reference.nii"));
            for run in 1..=2 {
                if gaps && sub == 2 && ses == 2 && run == 2 {
                    continue;
                }
                files.push(format!(
                    "sub-{sub:02}/ses-{ses}/anat/sub-{sub:02}_ses-{ses}_run-{run}_T1w.nii"
                ));
            }
        }
    }
    let files: Vec<_> = files.iter().map(String::as_str).collect();
    let tree = Tree::new("outputs", &files);
    tree.write("pipeline.spit", PIPELINE);
    tree.write(
        "strict.spitin",
        &format!("pipeline pipeline.spit\nroot .\n{DISCOVER}{STRICT}"),
    );
    tree.write(
        "drop.spitin",
        &format!("pipeline pipeline.spit\nroot .\n{DISCOVER}{DROP}"),
    );
    tree
}

/// `spit` run in `tree` with `args`: its exit code, standard output and
/// standard error, with the tree's folder written as `<root>`.
fn run(tree: &Tree, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .current_dir(tree.path())
        .output()
        .unwrap();
    let text = format!(
        "$ spit {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
        args.join(" "),
        output.status.code().unwrap_or(-1),
        support::text(&output.stdout),
        support::text(&output.stderr),
    );
    let root = tree.path();
    let canonical = root.canonicalize().unwrap();
    text.replace(&canonical.display().to_string(), "<root>")
        .replace(&root.display().to_string(), "<root>")
}

/// Compare `actual` with the fixture `name`, or write it with `SPIT_BLESS`.
fn check(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/outputs")
        .join(format!("{name}.txt"));
    if std::env::var_os("SPIT_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no fixture {}; run with SPIT_BLESS=1", path.display()));
    if actual != expected {
        let line = expected
            .lines()
            .zip(actual.lines())
            .position(|(expected, actual)| expected != actual)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        panic!(
            "{name} differs from {} at line {}:\nexpected: {:?}\nactual:   {:?}",
            path.display(),
            line + 1,
            expected.lines().nth(line),
            actual.lines().nth(line),
        );
    }
}

#[test]
fn a_complete_dataset_prints_as_before() {
    let tree = dataset(false);
    let runs: [&[&str]; 5] = [
        &["inputs", "strict.spitin", "-o", "complete.spitout"],
        &["dag", "strict.spitin", "--jobs"],
        &["dag", "strict.spitin", "--json"],
        &["dag", "strict.spitin", "--paths"],
        &["artifacts", "strict.spitin"],
    ];
    let mut text: String = runs.iter().map(|args| run(&tree, args)).collect();
    text.push_str(&std::fs::read_to_string(tree.path().join("complete.spitout")).unwrap());
    check("complete", &text);
    let spitout: [&[&str]; 2] = [
        &["dag", "pipeline.spit", "complete.spitout", "--jobs"],
        &["artifacts", "pipeline.spit", "complete.spitout"],
    ];
    let text: String = spitout.iter().map(|args| run(&tree, args)).collect();
    check("complete_spitout", &text);
}

#[test]
fn command_lines_print_as_before() {
    let tree = dataset(false);
    let runs: [&[&str]; 3] = [
        &["dag", "strict.spitin", "--commands"],
        &["dag", "strict.spitin", "--commands", "--paths"],
        // Plain `dag` prints the commands, as `--commands` does.
        &["dag", "strict.spitin"],
    ];
    let text: String = runs.iter().map(|args| run(&tree, args)).collect();
    check("commands", &text);
}

#[test]
fn a_dataset_with_gaps_prints_as_before() {
    let tree = dataset(true);
    let runs: [&[&str]; 5] = [
        &["dag", "strict.spitin"],
        &["artifacts", "strict.spitin"],
        &["inputs", "drop.spitin", "-o", "drop.spitout"],
        &["dag", "drop.spitin", "--jobs"],
        &["artifacts", "drop.spitin"],
    ];
    let mut text = String::new();
    for args in runs {
        text.push_str(&run(&tree, args));
        if args[0] == "inputs" {
            text.push_str(&std::fs::read_to_string(tree.path().join("drop.spitout")).unwrap());
        }
    }
    check("gaps", &text);
}

/// Diagnosing settled records in memory must give what writing them as a
/// .spitout and diagnosing that text gives, or nothing, so that the CLI
/// falls back to the text.
#[test]
fn recipes_diagnosed_in_memory_match_their_text() {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let cases = [
        (false, STRICT, false, true),
        (false, STRICT, true, true),
        (true, STRICT, true, true),
        (true, DROP, false, true),
        (true, DROP, true, true),
        // An unmet `require` is an error, which points at the text.
        (true, STRICT, false, false),
    ];
    for (gaps, rules, lenient, in_memory) in cases {
        let tree = dataset(gaps);
        let spec = parse_input_spec(&format!("{DISCOVER}{rules}")).unwrap();
        let settled = spec
            .resolve(&pipeline, InputSource::Discover(tree.path()))
            .unwrap();
        let context = Context {
            path: None,
            recipe: Some(&spec),
            lenient,
        };
        let text = render_source_inventory(&settled.inventory, &pipeline, &spec.rules);
        let from_text = diagnose_checked_with_records(PIPELINE, &text, context);
        let case = format!("gaps {gaps}, lenient {lenient}, rules {rules:?}");
        let Some((checked, records)) = diagnose_checked_with_inventory(PIPELINE, &settled, context)
        else {
            assert!(!in_memory, "{case}: expected to diagnose in memory");
            continue;
        };
        assert!(in_memory, "{case}: expected to diagnose the text");
        let (text_checked, text_records) = from_text.unwrap();
        assert_eq!(checked.warnings, text_checked.warnings, "{case}");
        assert_eq!(
            format!("{:?}", checked.pipeline),
            format!("{:?}", text_checked.pipeline),
            "{case}"
        );
        let sorted = |mut inventory: spit::SourceInventory| {
            inventory.artifacts.sort();
            inventory
        };
        assert_eq!(
            sorted(records.inventory),
            sorted(text_records.inventory),
            "{case}"
        );
        assert_eq!(
            render_artifacts(&records.report),
            render_artifacts(&text_records.report),
            "{case}"
        );
        assert_eq!(
            render_dag(&records.report.dag),
            render_dag(&text_records.report.dag),
            "{case}"
        );
    }
}
