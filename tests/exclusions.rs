//! `exclude` rules: removing named artifacts and groups from a dataset, in a
//! recipe or from a CSV file, and the record of what each removed.

mod support;

use support::{spit, text, Tree};

use spit::{
    parse_input_spec, parse_pipeline, parse_source_inventory, render_source_inventory, InputError,
    InputSource, InputSpec,
};

/// Sessions of runs, each session with one reference image.
const PIPELINE: &str = "\
source bold : Bold [sub, ses, run]
path bold: sub-{sub}/ses-{ses}/run-{run}.nii
source ref : Ref [sub, ses]
path ref: sub-{sub}/ses-{ses}/ref.nii
operation align(bold: Bold, ref: Ref) -> Bold
command align: align {bold} {ref} {output}
path aligned: out/sub-{sub}_ses-{ses}_run-{run}.nii
aligned = align(bold, ref)
operation average(runs: many Bold) -> Bold
command average: average {runs} {output}
path average: out/sub-{sub}_ses-{ses}_average.nii
average = average(aligned @ vary(run))
";

const FILES: [&str; 8] = [
    "sub-01/ses-01/run-1.nii",
    "sub-01/ses-01/run-2.nii",
    "sub-01/ses-01/ref.nii",
    "sub-02/ses-01/run-1.nii",
    "sub-02/ses-01/run-2.nii",
    "sub-02/ses-01/run-3.nii",
    "sub-02/ses-01/ref.nii",
    "sub-03/ses-01/run-1.nii",
];

/// A tree of `FILES` with the pipeline and a recipe of `rules`.
fn dataset(name: &str, rules: &str) -> Tree {
    let tree = Tree::new(name, &FILES);
    tree.write("pipeline.spit", PIPELINE);
    tree.write("data.spitin", &format!("pipeline pipeline.spit\n{rules}"));
    tree
}

/// `spit` run on the tree's recipe: its exit status, stdout and stderr.
fn run(tree: &Tree, command: &str) -> (bool, String, String) {
    let recipe = tree.path().join("data.spitin");
    let output = spit(&[command, recipe.to_str().unwrap()]);
    (
        output.status.success(),
        text(&output.stdout),
        text(&output.stderr),
    )
}

#[test]
fn an_exclude_names_an_artifact_a_group_or_part_of_a_source() {
    let spec = parse_input_spec(
        "exclude bold[sub=02,ses=01,run=3]  # corrupted: motion spike at #140\n\
         exclude [sub=03]\n\
         exclude bold[run=2]\n\
         exclude ref\n",
    )
    .unwrap();
    let rules: Vec<_> = spec
        .rules
        .exclusions
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        rules,
        [
            "exclude bold[sub=02,ses=01,run=3]",
            "exclude [sub=03]",
            "exclude bold[run=2]",
            "exclude ref",
        ]
    );
    let first = &spec.rules.exclusions[0];
    assert_eq!(
        first.reason.as_deref(),
        Some("corrupted: motion spike at #140")
    );
    assert_eq!(first.origin, "line 1");
    assert_eq!(spec.rules.exclusions[1].reason, None);

    for (rule, message) in [
        ("exclude\n", "expected source"),
        ("exclude from\n", "expected"),
        ("exclude []\n", "names a source, values"),
        ("exclude bold[sub=02\n", "closing `]`"),
    ] {
        let error = parse_input_spec(rule).unwrap_err().to_string();
        assert!(error.contains(message), "{rule}: {error}");
    }
    let error = parse_pipeline("source x [a]\nexclude x\n").unwrap_err();
    assert!(
        error.to_string().contains("belong in a .spitin recipe"),
        "{error}"
    );
}

#[test]
fn a_rule_is_checked_against_the_pipeline_without_data() {
    for (rule, message) in [
        ("exclude aligned[sub=01]", "`aligned` is not a source"),
        ("exclude ref[run=1]", "`run` is not a dimension of `ref`"),
        (
            "exclude [visit=1]",
            "`visit` is not a dimension of any source",
        ),
    ] {
        let tree = dataset("exclude-check", &format!("{rule}\n"));
        let (ok, _, stderr) = run(&tree, "check");
        assert!(!ok, "{rule}");
        assert!(stderr.contains(message), "{rule}: {stderr}");
        assert!(stderr.contains("line 2, column 9"), "{rule}: {stderr}");
    }
}

#[test]
fn an_excluded_run_leaves_its_session_to_the_other_runs() {
    let tree = dataset(
        "exclude-run",
        "exclude bold[sub=02,ses=01,run=3]  # corrupted\n",
    );
    let (ok, stdout, stderr) = run(&tree, "inputs");
    assert!(ok, "{stderr}");
    assert!(
        stderr.contains("note: excluded bold[sub=02,ses=01,run=3] (line 2): corrupted\n"),
        "{stderr}"
    );
    assert!(
        !stdout.contains("run=3") || stdout.contains("removed:"),
        "{stdout}"
    );
    assert!(
        stdout.ends_with(
            "removed:\n    bold[sub=02,ses=01,run=3]\n        rule: exclude bold[sub=02,ses=01,run=3]\n\
             \x20       at: line 2\n        reason: corrupted\n"
        ),
        "{stdout}"
    );
    // Subject 03 has a run but no reference, so the plan needs it excluded too.
    let (ok, _, stderr) = run(&tree, "dag");
    assert!(!ok);
    assert!(
        stderr.contains("no `ref` artifact for input `ref` of `align` at [run=1,ses=01,sub=03]"),
        "{stderr}"
    );
    tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nexclude bold[sub=02,ses=01,run=3]  # corrupted\nexclude [sub=03]\n",
    );
    let recipe = tree.path().join("data.spitin");
    let output = spit(&["dag", recipe.to_str().unwrap(), "--commands"]);
    let stdout = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(!stdout.contains("run-3"), "{stdout}");
    assert!(!stdout.contains("sub-03"), "{stdout}");
    assert!(
        stdout.contains("average out/sub-02_ses-01_run-1.nii out/sub-02_ses-01_run-2.nii out/sub-02_ses-01_average.nii"),
        "{stdout}"
    );
}

#[test]
fn a_group_is_removed_from_every_source_and_recorded_once() {
    let tree = dataset(
        "exclude-group",
        "exclude [sub=02]  # withdrew\nexclude [sub=03]\n",
    );
    let (ok, stdout, stderr) = run(&tree, "inputs");
    assert!(ok, "{stderr}");
    let sources = &stdout[..stdout.find("removed:").unwrap()];
    assert!(!sources.contains("sub=02"), "{stdout}");
    assert_eq!(stdout.matches("\n    [sub=02]\n").count(), 1, "{stdout}");
    assert!(stdout.contains("    [sub=02]\n        rule: exclude [sub=02]\n        at: line 2\n        reason: withdrew\n"), "{stdout}");
}

#[test]
fn an_exclude_that_matches_nothing_is_an_error_naming_close_values() {
    let tree = dataset("exclude-unmatched", "exclude bold[sub=2,ses=01,run=3]\n");
    let (ok, _, stderr) = run(&tree, "inputs");
    assert!(!ok);
    assert!(
        stderr.contains(
            "error: `exclude bold[sub=2,ses=01,run=3]` (line 2) matches nothing in this dataset; it has sub=02"
        ),
        "{stderr}"
    );
}

#[test]
fn records_given_directly_are_excluded_too() {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let records = parse_source_inventory(
        "sources:\n    bold[sub=01,ses=01,run=1]\n    bold[sub=01,ses=01,run=2]\n    ref[sub=01,ses=01]\n",
    )
    .unwrap();
    let spec = parse_input_spec("exclude bold[run=2]\n").unwrap();
    let settled = spec
        .resolve(&pipeline, InputSource::Inventory(records.clone()))
        .unwrap();
    assert_eq!(settled.inventory.artifacts.len(), 2);
    assert_eq!(settled.inventory.removed.len(), 1);
    assert_eq!(
        settled.inventory.removed[0].identity(),
        "bold[run=2,ses=01,sub=01]"
    );

    let spec = parse_input_spec("exclude bold[run=9]\n").unwrap();
    let error = spec
        .resolve(&pipeline, InputSource::Inventory(records))
        .unwrap_err();
    assert!(
        matches!(error, InputError::UnmatchedExclusion(_)),
        "{error}"
    );
}

#[test]
fn a_discovered_context_left_out_needs_no_files() {
    // Session 02 of subject 01 has no files at all; excluding it lets the
    // discovery of every session succeed.
    let tree = dataset(
        "exclude-context",
        "discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}\n\
         exclude [sub=01,ses=02]  # scan never happened\n\
         exclude [sub=03]\n",
    );
    std::fs::create_dir_all(tree.path().join("sub-01/ses-02")).unwrap();
    let (ok, stdout, stderr) = run(&tree, "inputs");
    assert!(ok, "{stderr}");
    assert!(!stdout.contains("[sub=01,ses=02]:"), "{stdout}");
    assert!(
        stdout.contains("    [sub=01,ses=02]\n        rule: exclude [sub=01,ses=02]"),
        "{stdout}"
    );
    // Without the exclusion, the empty session's reference is missing.
    tree.write(
        "data.spitin",
        "pipeline pipeline.spit\ndiscover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}\nexclude [sub=03]\n",
    );
    let (ok, _, stderr) = run(&tree, "inputs");
    assert!(!ok);
    assert!(
        stderr.contains("missing source file for `ref[ses=02,sub=01]`"),
        "{stderr}"
    );
}

#[test]
fn an_expected_file_excluded_by_name_need_not_exist() {
    let tree = dataset(
        "exclude-missing",
        "discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}\n\
         exclude ref[sub=01,ses=02]  # never acquired\n\
         exclude [sub=03]\n",
    );
    tree.write("sub-01/ses-02/run-1.nii", "");
    let (ok, stdout, stderr) = run(&tree, "inputs");
    assert!(ok, "{stderr}");
    assert!(
        stderr.contains("note: excluded ref[sub=01,ses=02] (line 3): never acquired"),
        "{stderr}"
    );
    assert!(stdout.contains("    ref[sub=01,ses=02]\n        rule: exclude ref[sub=01,ses=02]\n        at: line 3\n"), "{stdout}");
}

#[test]
fn an_excluded_file_may_lie_outside_the_discovered_contexts() {
    // Stores are discovered from their folders. Store s07's price list was
    // saved as `S07.json`, a store no folder has, which stops discovery.
    let tree = Tree::new(
        "exclude-stray",
        &[
            "stores/s01/sales.csv",
            "stores/s07/sales.csv",
            "pricing/s01.json",
            "pricing/S07.json",
        ],
    );
    tree.write(
        "pipeline.spit",
        "source sales [store]\npath sales: stores/{store}/sales.csv\n\
         source price [store]\npath price: pricing/{store}.json\n\
         operation total(sales, price) -> Total\ncommand total: total {sales} {price} {output}\n\
         totals = total(sales, price)\n",
    );
    let discover = "pipeline pipeline.spit\ndiscover stores: [store] from dirs stores/{store}\n";
    tree.write("data.spitin", discover);
    let (ok, _, stderr) = run(&tree, "inputs");
    assert!(!ok);
    assert!(
        stderr.contains("`pricing/S07.json` for `price` lies outside the discovered contexts"),
        "{stderr}"
    );

    // Excluding the misnamed file lets discovery go on, to the price list
    // store s07 lacks; excluding the store as well settles the dataset.
    tree.write(
        "data.spitin",
        &format!("{discover}exclude price[store=S07]  # misnamed\n"),
    );
    let (ok, stdout, stderr) = run(&tree, "inputs");
    assert!(!ok, "unexpected inventory: {stdout}\nstderr: {stderr}");
    assert!(
        stderr.contains("missing source file for `price[store=s07]`"),
        "{stderr}"
    );
    tree.write(
        "data.spitin",
        &format!("{discover}exclude price[store=S07]  # misnamed\nexclude [store=s07]\n"),
    );
    let (ok, stdout, stderr) = run(&tree, "inputs");
    assert!(ok, "{stderr}");
    assert!(
        stdout.contains("    price[store=S07]\n        rule: exclude price[store=S07]"),
        "{stdout}"
    );
    assert!(
        stdout.contains("    [store=s07]\n        rule: exclude [store=s07]"),
        "{stdout}"
    );
}

#[test]
fn a_failed_join_names_the_exclusion_that_caused_it() {
    let tree = dataset(
        "excluded-join",
        "exclude ref[sub=01,ses=01]  # rejected image\n",
    );
    let (ok, _, stderr) = run(&tree, "dag");
    assert!(!ok);
    assert!(
        stderr.contains("ref[ses=01,sub=01] was excluded by recipe line 2"),
        "{stderr}"
    );
    assert!(stderr.contains("--partial"), "{stderr}");
}

#[test]
fn rules_are_read_from_a_csv_file_beside_the_recipe() {
    let tree = dataset("exclude-csv", "exclude from qc/excluded.csv\n");
    tree.write(
        "qc/excluded.csv",
        "product,sub,ses,run,reason\nbold,02,01,3,\"motion spike, volume 140\"\n,03,,,no reference\n",
    );
    let (ok, stdout, stderr) = run(&tree, "inputs");
    assert!(ok, "{stderr}");
    assert!(
        stderr.contains("note: excluded bold[sub=02,ses=01,run=3] (qc/excluded.csv line 2): motion spike, volume 140"),
        "{stderr}"
    );
    assert!(
        stdout.contains("        at: qc/excluded.csv line 3\n        reason: no reference\n"),
        "{stdout}"
    );

    tree.write("qc/excluded.csv", "product,sub,ses,run\nbold,02,01,9\n");
    let (ok, _, stderr) = run(&tree, "inputs");
    assert!(!ok);
    assert!(
        stderr.contains("(qc/excluded.csv line 2) matches nothing"),
        "{stderr}"
    );

    tree.write("qc/excluded.csv", "product,subject\nbold,02\n");
    let (ok, _, stderr) = run(&tree, "check");
    assert!(!ok);
    assert!(
        stderr.contains("(qc/excluded.csv line 2): `subject` is not a dimension of `bold`"),
        "{stderr}"
    );

    std::fs::remove_file(tree.path().join("qc/excluded.csv")).unwrap();
    let (ok, _, stderr) = run(&tree, "inputs");
    assert!(!ok);
    assert!(
        stderr.contains("cannot read `qc/excluded.csv` for `exclude from`"),
        "{stderr}"
    );
}

#[test]
fn the_record_of_what_was_removed_reads_back_and_reaches_the_spitdag() {
    let tree = dataset(
        "exclude-record",
        "exclude bold[sub=02,ses=01,run=3]  # corrupted: see #140\nexclude [sub=03]\n",
    );
    let recipe = tree.path().join("data.spitin");
    let spitout = tree.path().join("data.spitout");
    let output = spit(&[
        "inputs",
        recipe.to_str().unwrap(),
        "-o",
        spitout.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let written = std::fs::read_to_string(&spitout).unwrap();
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let read = parse_source_inventory(&written).unwrap();
    assert_eq!(read.removed.len(), 2);
    assert_eq!(
        read.removed[0].reason.as_deref(),
        Some("corrupted: see #140")
    );
    assert_eq!(
        render_source_inventory(&read, &pipeline, &InputSpec::default().rules),
        written
    );

    let pipeline_file = tree.path().join("pipeline.spit");
    let output = spit(&[
        "dag",
        pipeline_file.to_str().unwrap(),
        spitout.to_str().unwrap(),
        "--json",
    ]);
    let json = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        json.contains(
            "\"removed\":[{\"product\":\"bold\",\"entities\":{\"run\":\"3\",\"ses\":\"01\",\"sub\":\"02\"},\
\"rule\":\"exclude bold[sub=02,ses=01,run=3]\",\"origin\":\"line 2\",\"reason\":\"corrupted: see #140\",\"found\":null},\
{\"product\":null,\"entities\":{\"sub\":\"03\"},\"rule\":\"exclude [sub=03]\",\"origin\":\"line 3\",\"reason\":null,\"found\":null}]"
        ),
        "{json}"
    );
}
