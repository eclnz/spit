//! The `.spitin` input stage: a recipe's discovery, `require`, `skip` and
//! source path rules are settled before jobs are resolved, so resolving jobs
//! sees only the logical pipeline and a plain inventory.

use std::fs;
use std::process::Command;

use spit::{parse_input_spec, parse_pipeline, resolve, InputSource, ResolveError, SourceInventory};

mod support;
use support::Tree;

const PIPELINE: &str = "\
source image: Image [sub, ses]
operation process(Image) -> Image
result = process(image)
";

const RECIPE: &str = "\
discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
";

const FILES: [&str; 3] = [
    "data/sub-1/ses-1/image.nii.gz",
    "data/sub-1/ses-2/image.nii.gz",
    "data/sub-5/ses-1/image.nii.gz",
];

fn inventory_of(recipe: &str, tree: &Tree) -> spit::ResolvedInputs {
    parse_input_spec(recipe)
        .unwrap()
        .resolve(
            &parse_pipeline(PIPELINE).unwrap(),
            InputSource::Discover(tree.path()),
        )
        .unwrap()
}

#[test]
fn the_stage_finds_contexts_and_sources_without_touching_the_pipeline() {
    let tree = Tree::new("finds", &FILES);
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let resolved = parse_input_spec(RECIPE)
        .unwrap()
        .resolve(&pipeline, InputSource::Discover(tree.path()))
        .unwrap();
    assert_eq!(resolved.inventory.contexts.len(), 3);
    assert_eq!(resolved.inventory.artifacts.len(), 3);
    assert_eq!(resolved.inventory.discovered["sessions"].len(), 3);
    assert!(resolved.gaps.is_empty());
    assert!(pipeline.product_paths.is_empty());
}

#[test]
fn jobs_resolve_from_the_logical_pipeline_and_the_stages_inventory() {
    let tree = Tree::new("dag", &FILES);
    let resolved = inventory_of(RECIPE, &tree);
    let inventory = resolved.dag_inventory();
    assert!(inventory.discovered.is_empty());
    let dag = resolve(&parse_pipeline(PIPELINE).unwrap(), &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 3);
}

#[test]
fn skip_rules_are_applied_by_the_stage_before_jobs_exist() {
    let tree = Tree::new("skip", &FILES);
    let recipe = format!("{RECIPE}skip sessions count>=2 per [sub]\n");
    let resolved = inventory_of(&recipe, &tree);
    assert_eq!(resolved.inventory.artifacts.len(), 2);
    assert!(resolved
        .skipped
        .iter()
        .any(|note| note.contains("skip sessions")));
    let dag = resolve(
        &parse_pipeline(PIPELINE).unwrap(),
        &resolved.dag_inventory(),
    )
    .unwrap();
    assert_eq!(dag.jobs.len(), 2);
}

#[test]
fn require_gaps_are_the_stages_to_report_and_do_not_reach_the_resolver() {
    let tree = Tree::new("missing", &FILES);
    let recipe = format!("{RECIPE}require sessions count>=2 per [sub]\n");
    let resolved = inventory_of(&recipe, &tree);
    assert_eq!(resolved.gaps.len(), 1);
    assert!(matches!(
        resolved.require_complete(),
        Err(ResolveError::CoverageViolation {
            found: 1,
            discovery: true,
            ..
        })
    ));
    // The resolver knows no requirement, so it resolves what the stage left.
    let dag = resolve(
        &parse_pipeline(PIPELINE).unwrap(),
        &resolved.dag_inventory(),
    )
    .unwrap();
    assert_eq!(dag.jobs.len(), 3);
}

#[test]
fn a_recipe_can_settle_records_that_were_already_written() {
    let recipe =
        parse_input_spec("path image: data/{sub}/{ses}.nii.gz\nrequire image count>=2 per [sub]\n")
            .unwrap();
    let inventory = spit::parse_source_inventory(
        "sources:\n  image[sub=1, ses=1]\n  image[sub=2, ses=1]\n  image[sub=2, ses=2]\n",
    )
    .unwrap();
    let resolved = recipe
        .resolve(
            &parse_pipeline(PIPELINE).unwrap(),
            InputSource::Inventory(inventory),
        )
        .unwrap();
    assert_eq!(resolved.gaps.len(), 1);
}

#[test]
fn skipping_written_records_reports_the_removed_group() {
    let recipe = parse_input_spec("skip image count>=2 per [sub]\n").unwrap();
    let inventory = spit::parse_source_inventory(
        "sources:\n  image[sub=1,ses=1]\n  image[sub=2,ses=1]\n  image[sub=2,ses=2]\n",
    )
    .unwrap();
    let resolved = recipe
        .resolve(
            &parse_pipeline(PIPELINE).unwrap(),
            InputSource::Inventory(inventory),
        )
        .unwrap();
    assert_eq!(resolved.inventory.artifacts.len(), 2);
    assert_eq!(
        resolved.skipped,
        ["[sub=1] because `skip image` rejected the group"]
    );
}

#[test]
fn discover_scans_the_root_when_a_recipe_has_written_records() {
    let tree = Tree::new("discover-records", &FILES);
    let pipeline = tree.path().join("analysis.spit");
    let recipe = tree.path().join("analysis.spitin");
    fs::write(&pipeline, PIPELINE).unwrap();
    fs::write(
        &recipe,
        format!("{RECIPE}sources:\n  image[sub=old,ses=1]: data/old/image.nii.gz\n"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "discover",
            pipeline.to_str().unwrap(),
            "--root",
            tree.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inventory = String::from_utf8(output.stdout).unwrap();
    assert!(inventory.contains("image[sub=1,ses=1]"), "{inventory}");
    assert!(!inventory.contains("sub=old"), "{inventory}");
}

#[test]
fn a_recipe_path_for_an_unknown_source_is_rejected_by_the_stage() {
    let tree = Tree::new("unknown", &FILES);
    let recipe = parse_input_spec(&format!("{RECIPE}path other: x/{{sub}}.txt\n")).unwrap();
    let error = recipe
        .resolve(
            &parse_pipeline(PIPELINE).unwrap(),
            InputSource::Discover(tree.path()),
        )
        .unwrap_err();
    assert!(error.to_string().contains("must name a source product"));
}

#[test]
fn an_empty_inventory_is_still_a_valid_stage_result() {
    let recipe = parse_input_spec("path image: data/{sub}/{ses}.nii.gz\n").unwrap();
    let resolved = recipe
        .resolve(
            &parse_pipeline(PIPELINE).unwrap(),
            InputSource::Inventory(SourceInventory::default()),
        )
        .unwrap();
    assert!(resolved.inventory.artifacts.is_empty());
}

#[test]
fn sibling_spitin_discovers_inputs_and_defaults_output_paths() {
    let tree = Tree::new(
        "spitin-sibling",
        &[
            "data/sub-1/ses-1/image.nii.gz",
            "data/sub-1/ses-2/image.nii.gz",
            "data/sub-5/ses-1/.keep",
        ],
    );
    let pipeline_file = tree.0.join("analysis.spit");
    fs::write(
        &pipeline_file,
        "source image: Image [sub, ses]\n\
         operation process(Image) -> Image\n\
         result = process(image)\n",
    )
    .unwrap();
    fs::write(
        tree.0.join("analysis.spitin"),
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         skip sessions count>=2 per [sub]\n\
         require sessions count>=2 per [sub]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n",
    )
    .unwrap();
    let check = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stderr).contains("skip sessions"));
    assert!(String::from_utf8_lossy(&check.stdout).contains("2 jobs resolved"));
    let paths = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["dag", pipeline_file.to_str().unwrap(), "--paths"])
        .output()
        .unwrap();
    assert!(
        paths.status.success(),
        "{}",
        String::from_utf8_lossy(&paths.stderr)
    );
    let paths = String::from_utf8(paths.stdout).unwrap();
    assert!(paths.contains("out/result/sub=1__ses=1"), "{paths}");
    assert!(!paths.contains("sub=5"), "{paths}");
}

#[test]
fn explicit_spitin_uses_its_own_directory_and_require_reports_gaps() {
    let tree = Tree::new(
        "spitin-explicit",
        &[
            "dataset/data/sub-1/ses-1/image.nii.gz",
            "dataset/data/sub-1/ses-2/image.nii.gz",
            "dataset/data/sub-5/ses-1/image.nii.gz",
        ],
    );
    let pipeline_file = tree.0.join("pipeline.spit");
    fs::write(
        &pipeline_file,
        "source image [sub, ses]\n\
         operation process(Image) -> Image\n\
         result = process(image)\n",
    )
    .unwrap();
    let recipe = tree.0.join("dataset/inputs.spitin");
    fs::write(
        &recipe,
        "discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         require sessions count>=2 per [sub]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n",
    )
    .unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            pipeline_file.to_str().unwrap(),
            "--inputs",
            recipe.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stderr)
        .contains("discovery coverage for `sessions` at [sub=5]"));
}

#[test]
fn the_stage_writes_each_sources_path_into_its_record() {
    let tree = Tree::new("paths", &FILES);
    let resolved = inventory_of(RECIPE, &tree);
    let paths: Vec<_> = resolved
        .inventory
        .artifacts
        .iter()
        .map(|record| record.path.as_deref().unwrap())
        .collect();
    assert_eq!(paths, FILES);
    // Records written without paths get the one the recipe's rule gives.
    let written = spit::parse_source_inventory("sources:\n  image[sub=2, ses=1]\n").unwrap();
    let located = parse_input_spec(RECIPE)
        .unwrap()
        .resolve(
            &parse_pipeline(PIPELINE).unwrap(),
            InputSource::Inventory(written),
        )
        .unwrap();
    assert_eq!(
        located.inventory.artifacts[0].path.as_deref(),
        Some("data/sub-2/ses-1/image.nii.gz")
    );
}

#[test]
fn a_spitout_alone_drives_jobs_without_its_recipe() {
    let tree = Tree::new("spitout", &FILES);
    // Sources have no path rule in the pipeline: only the recipe knows them.
    let pipeline = tree.path().join("analysis.spit");
    fs::write(
        &pipeline,
        format!("{PIPELINE}path result: results/{{sub}}_{{ses}}.nii.gz\ncommand process: tool {{input}} {{output}}\n"),
    )
    .unwrap();
    let recipe = tree.path().join("dataset.spitin");
    fs::write(&recipe, RECIPE).unwrap();
    let spit = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .output()
            .unwrap()
    };
    let (pipeline, recipe) = (pipeline.to_str().unwrap(), recipe.to_str().unwrap());
    let discovered = spit(&["discover", pipeline, "--inputs", recipe]);
    assert!(
        discovered.status.success(),
        "{}",
        String::from_utf8_lossy(&discovered.stderr)
    );
    let spitout = String::from_utf8(discovered.stdout).unwrap();
    assert!(
        spitout.contains("image[sub=1,ses=2]: data/sub-1/ses-2/image.nii.gz"),
        "{spitout}"
    );
    // Step 3 from the .spitout and the pipeline, with the recipe removed.
    let saved = tree.path().join("dataset.spitout");
    fs::write(&saved, &spitout).unwrap();
    fs::remove_file(recipe).unwrap();
    let saved = saved.to_str().unwrap();
    let root = tree.path().to_str().unwrap();
    let dag = spit(&["dag", pipeline, "--sources", saved, "--paths"]);
    assert!(
        dag.status.success(),
        "{}",
        String::from_utf8_lossy(&dag.stderr)
    );
    let dag = String::from_utf8(dag.stdout).unwrap();
    assert!(dag.contains("data/sub-5/ses-1/image.nii.gz"), "{dag}");
    assert!(dag.contains("results/5_1.nii.gz"), "{dag}");
    let checked = spit(&[
        "check",
        pipeline,
        "--sources",
        saved,
        "--root",
        root,
        "--paths",
    ]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let report = String::from_utf8(checked.stdout).unwrap();
    assert!(
        report.contains("image (source): from the inventory"),
        "{report}"
    );
    assert!(report.contains("3 source files verified."), "{report}");
    let bash = spit(&["bash", pipeline, "--sources", saved]);
    assert!(
        bash.status.success(),
        "{}",
        String::from_utf8_lossy(&bash.stderr)
    );
    assert!(String::from_utf8(bash.stdout)
        .unwrap()
        .contains("'data/sub-1/ses-1/image.nii.gz'"));
}
