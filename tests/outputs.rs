mod support;

use std::process::Command;

use spit::{
    diagnose_checked_with_inventory, diagnose_checked_with_records, parse_input_spec,
    parse_pipeline, render_artifacts, render_dag, render_source_inventory, Completeness, Context,
    InputSource,
};
use support::Tree;

const PIPELINE: &str = "\
default path: derivatives/{@product}/{@entities}.txt
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
command average: mean {images} --runs {images.run} -o {@output}
averaged = average(aligned @vary(run))

operation compare(image: Image<$K>, reference: Reference) -> Score
command compare: score {image} {reference} --label \"sub-{sub} ses-{ses}\" -o {@output}
score = compare(averaged, reference)
default entities: {@key}={@value} separated \"__\"\n";

const DISCOVER: &str = "inputs:\n    contexts: sub-{sub}/ses-{ses}\n";
const STRICT: &str = "\
require [sub, ses] where image count>=2
require [sub, ses] where reference count=1
";
const DROP: &str = "\
exclude [sub, ses] where image count<2
require [sub, ses] where reference count=1
";

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
    support::check_output("complete", &text);
    let spitout: [&[&str]; 2] = [
        &["dag", "pipeline.spit", "complete.spitout", "--jobs"],
        &["artifacts", "pipeline.spit", "complete.spitout"],
    ];
    let text: String = spitout.iter().map(|args| run(&tree, args)).collect();
    support::check_output("complete_spitout", &text);
}

#[test]
fn command_lines_print_as_before() {
    let tree = dataset(false);
    let runs: [&[&str]; 3] = [
        &["dag", "strict.spitin", "--commands"],
        &["dag", "strict.spitin", "--commands", "--paths"],
        &["dag", "strict.spitin"],
    ];
    let text: String = runs.iter().map(|args| run(&tree, args)).collect();
    support::check_output("commands", &text);
}

#[test]
fn a_dataset_with_gaps_prints_as_before() {
    let tree = dataset(true);
    let runs: [&[&str]; 6] = [
        &["dag", "strict.spitin"],
        &["artifacts", "strict.spitin"],
        &["artifacts", "strict.spitin", "--by-target"],
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
    support::check_output("gaps", &text);
}

#[test]
fn recipes_diagnosed_in_memory_match_their_text() {
    let cases = [
        (false, STRICT, false, true),
        (false, STRICT, true, true),
        (true, STRICT, true, true),
        (true, DROP, false, true),
        (true, DROP, true, true),
        (true, STRICT, false, false),
    ];
    for (gaps, rules, lenient, in_memory) in cases {
        let tree = dataset(gaps);
        let case = format!("gaps {gaps}, lenient {lenient}, rules {rules:?}");
        let recipe = format!("{DISCOVER}{rules}");
        let diagnosed = same_in_memory_as_in_text(PIPELINE, &recipe, &tree, lenient, &case);
        assert_eq!(diagnosed, in_memory, "{case}: diagnosed in memory");
    }
}

#[test]
fn recipe_source_paths_and_reviews_diagnose_in_memory_as_in_text() {
    let without_mask_path = PIPELINE.replace("path mask: sub-{sub}/mask.nii\n", "");
    let reviewed = PIPELINE.replace(
        "cleaned = clean(image, mask)",
        "cleaned = clean(image, mask) @review",
    );
    for gaps in [false, true] {
        let tree = dataset(gaps);
        let named = format!("{DISCOVER}    mask: sub-{{sub}}/mask.nii\n{DROP}");
        let case = format!("gaps {gaps}, the recipe names the mask's path");
        assert!(
            same_in_memory_as_in_text(&without_mask_path, &named, &tree, false, &case),
            "{case}"
        );
        let verdicts = tree.write("looked_at.csv", "sub,verdict,reason\n01,fail,blurred\n");
        let review = format!(
            "{DISCOVER}{DROP}review cleaned from {}\n",
            verdicts.display()
        );
        let case = format!("gaps {gaps}, a review with a verdict");
        assert!(
            same_in_memory_as_in_text(&reviewed, &review, &tree, false, &case),
            "{case}"
        );
    }
}

fn same_in_memory_as_in_text(
    pipeline_text: &str,
    recipe: &str,
    tree: &Tree,
    lenient: bool,
    case: &str,
) -> bool {
    let pipeline = parse_pipeline(pipeline_text).unwrap();
    let spec = parse_input_spec(recipe).unwrap();
    let settled = spec
        .resolve(&pipeline, InputSource::Discover(tree.path()))
        .unwrap();
    let context = Context {
        path: None,
        recipe: Some(&spec),
        completeness: if lenient {
            Completeness::Reported
        } else {
            Completeness::Required
        },
    };
    let text = render_source_inventory(&settled.inventory, &pipeline, &spec.rules);
    let from_text = diagnose_checked_with_records(pipeline_text, &text, context);
    let Some((checked, records)) =
        diagnose_checked_with_inventory(pipeline_text, &settled, context)
    else {
        return false;
    };
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
        render_artifacts(&checked.pipeline, &records.report),
        render_artifacts(&text_checked.pipeline, &text_records.report),
        "{case}"
    );
    assert_eq!(
        render_dag(&records.report.dag),
        render_dag(&text_records.report.dag),
        "{case}"
    );
    true
}
