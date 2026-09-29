//! The `.spitin` input stage: a recipe's discovery, `require`, `skip` and
//! source path rules are settled before jobs are resolved, so resolving jobs
//! sees only the logical pipeline and a plain inventory.

mod support;

use support::Tree;

use std::fs;
use std::process::Command;

use spit::{parse_input_spec, parse_pipeline, resolve, InputSource, ResolveError, SourceInventory};

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
    assert!(resolved.root.is_none());
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
fn a_given_root_is_scanned_even_when_the_recipe_has_records() {
    let tree = Tree::new("inputs-records", &FILES);
    tree.write("analysis.spit", PIPELINE);
    let recipe = tree.write(
        "analysis.spitin",
        &format!("pipeline analysis.spit\n{RECIPE}sources:\n  image[sub=old,ses=1]\n"),
    );
    let run = |root: Option<&str>| {
        let mut args = vec!["inputs", recipe.to_str().unwrap()];
        args.extend(root.iter().flat_map(|root| ["--root", *root]));
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .output()
            .unwrap()
    };
    // Without a root, the written records are the inputs.
    let records = run(None);
    let records = String::from_utf8(records.stdout).unwrap();
    assert!(records.contains("sub=old"), "{records}");
    // Given one, the scan replaces them.
    let scanned = run(tree.path().to_str());
    assert!(
        scanned.status.success(),
        "{}",
        String::from_utf8_lossy(&scanned.stderr)
    );
    let scanned = String::from_utf8(scanned.stdout).unwrap();
    assert!(
        scanned.contains("[sub=1,ses=1]:\n        image"),
        "{scanned}"
    );
    assert!(!scanned.contains("sub=old"), "{scanned}");
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
fn a_named_recipe_runs_in_memory_and_defaults_output_paths() {
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
        "pipeline analysis.spit\n\
         discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         skip sessions count>=2 per [sub]\n\
         require sessions count>=2 per [sub]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n",
    )
    .unwrap();
    // A recipe beside the pipeline is not loaded unless named.
    let check = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", pipeline_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&check.stdout), "Pipeline valid.\n");
    // Given in place of a .spitout, it runs the input stage in memory over
    // the pipeline it names, and outputs with no rule take the built-in layout.
    let recipe = tree.0.join("analysis.spitin");
    let paths = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["dag", recipe.to_str().unwrap(), "--paths"])
        .output()
        .unwrap();
    let notes = String::from_utf8_lossy(&paths.stderr).into_owned();
    assert!(paths.status.success(), "{notes}");
    assert!(notes.contains("skip sessions"), "{notes}");
    assert!(notes.contains("note: 2 jobs resolved."), "{notes}");
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
        "pipeline ../pipeline.spit\n\
         discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}\n\
         require sessions count>=2 per [sub]\n\
         path image: data/sub-{sub}/ses-{ses}/image.nii.gz\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .output()
            .unwrap()
    };
    let recipe = recipe.to_str().unwrap();
    // The recipe's `pipeline` line is relative to the recipe's folder, as
    // are the folders it scans.
    let checked = run(&["check", recipe]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    for args in [&["inputs", recipe][..], &["dag", recipe]] {
        let output = run(args);
        assert!(!output.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("discovery coverage for `sessions` at [sub=5]"),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
fn a_spitout_writes_values_in_the_declared_dimension_order() {
    let tree = Tree::new("order", &FILES);
    let resolved = inventory_of(RECIPE, &tree);
    let text = spit::render_source_inventory(
        &resolved.inventory,
        &parse_pipeline(PIPELINE).unwrap(),
        &Default::default(),
    );
    // `image` declares [sub, ses]: its records and the contexts follow suit.
    assert!(text.contains("contexts sessions:\n"), "{text}");
    assert!(text.contains("    image[sub=1,ses=1]\n"), "{text}");
}

#[test]
fn a_discovered_spitout_groups_sources_by_session() {
    let pipeline = parse_pipeline(
        "source image: Image [sub, ses, run]\nsource t1w: Image [sub, ses]\nsource lut: Table []\n",
    )
    .unwrap();
    let recipe =
        parse_input_spec("discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}\n").unwrap();
    let inventory = spit::parse_source_inventory(
        "contexts sessions:\n    [sub=02,ses=01]\n    [sub=01,ses=02]\n    [sub=01,ses=01]\n\
sources:\n    image[sub=02,ses=01,run=01]\n\
    image[sub=01,ses=02,run=01]\n\
    image[sub=01,ses=01,run=02]\n\
    lut[]\n\
    t1w[sub=01,ses=01]\n\
    image[sub=01,ses=01,run=01]\n",
    )
    .unwrap();
    let rendered = spit::render_source_inventory(&inventory, &pipeline, &recipe.rules);
    let shared = rendered.find("    lut\n").unwrap();
    let first = rendered.find("    [sub=01,ses=01]:").unwrap();
    let second = rendered.find("    [sub=01,ses=02]:").unwrap();
    let third = rendered.find("    [sub=02,ses=01]:").unwrap();
    assert!(
        shared < first && first < second && second < third,
        "{rendered}"
    );
    assert!(rendered.contains("[run=01,02]:"), "{rendered}");
    let reparsed = spit::parse_source_inventory(&rendered).unwrap();
    let mut actual = reparsed.artifacts;
    let mut expected = inventory.artifacts;
    actual.sort_by(|a, b| a.product.cmp(&b.product).then(a.entities.cmp(&b.entities)));
    expected.sort_by(|a, b| a.product.cmp(&b.product).then(a.entities.cmp(&b.entities)));
    assert_eq!(actual, expected);
    assert_eq!(reparsed.discovered, inventory.discovered);
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
    fs::write(&recipe, format!("pipeline analysis.spit\n{RECIPE}")).unwrap();
    let spit = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .output()
            .unwrap()
    };
    let (pipeline, recipe) = (pipeline.to_str().unwrap(), recipe.to_str().unwrap());
    let saved = tree.path().join("dataset.spitout");
    let saved = saved.to_str().unwrap();
    let written = spit(&["inputs", recipe, "-o", saved]);
    assert!(
        written.status.success(),
        "{}",
        String::from_utf8_lossy(&written.stderr)
    );
    let spitout = fs::read_to_string(saved).unwrap();
    assert!(
        spitout.contains("source_paths:\n    image: data/sub-{sub}/ses-{ses}/image.nii.gz"),
        "{spitout}"
    );
    assert!(
        spitout.contains("[sub=1,ses=2]:\n        image"),
        "{spitout}"
    );
    // Step 3 from the .spitout and the pipeline, with the recipe removed.
    fs::remove_file(recipe).unwrap();
    let root = tree.path().to_str().unwrap();
    let dag = spit(&["dag", pipeline, saved, "--paths"]);
    assert!(
        dag.status.success(),
        "{}",
        String::from_utf8_lossy(&dag.stderr)
    );
    let dag = String::from_utf8(dag.stdout).unwrap();
    assert!(dag.contains("data/sub-5/ses-1/image.nii.gz"), "{dag}");
    assert!(dag.contains("results/5_1.nii.gz"), "{dag}");
    // The records give every source its file, so no source needs a rule.
    let checked = spit(&["dag", pipeline, saved, "--root", root, "--strict-paths"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let notes = String::from_utf8(checked.stderr).unwrap();
    assert!(notes.contains("3 source files verified."), "{notes}");
}

#[test]
fn a_record_cannot_redirect_one_source_away_from_its_path_rule() {
    let pipeline =
        parse_pipeline("source image: Image [sub]\npath image: data/sub-{sub}/image.nii.gz\n")
            .unwrap();
    let inventory =
        spit::parse_source_inventory("sources:\n    image[sub=01]: elsewhere/image.nii.gz\n")
            .unwrap();
    let error = parse_input_spec("")
        .unwrap()
        .resolve(&pipeline, InputSource::Inventory(inventory))
        .unwrap_err();
    assert!(
        error.to_string().contains("differs from its path rule"),
        "{error}"
    );

    let tree = Tree::new("reject-record-path", &[]);
    let pipeline_file = tree.write(
        "pipeline.spit",
        "source image: Image [sub]\npath image: data/sub-{sub}/image.nii.gz\n",
    );
    let inputs_file = tree.write(
        "inputs.spitout",
        "sources:\n    image[sub=01]: elsewhere/image.nii.gz\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            pipeline_file.to_str().unwrap(),
            inputs_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("differs from its path rule"));
}

#[test]
fn a_spitout_source_path_rule_cannot_override_the_pipeline() {
    let pipeline =
        parse_pipeline("source image: Image [sub]\npath image: data/sub-{sub}/image.nii.gz\n")
            .unwrap();
    let inventory = spit::parse_source_inventory(
        "source_paths:\n    image: elsewhere/{sub}.nii.gz\nsources:\n    image[sub=01]\n",
    )
    .unwrap();
    let error = parse_input_spec("")
        .unwrap()
        .resolve(&pipeline, InputSource::Inventory(inventory))
        .unwrap_err();
    assert!(
        error.to_string().contains("both .spit and .spitout"),
        "{error}"
    );
}

#[test]
fn contexts_follow_the_discover_rule_and_list_in_the_order_written() {
    // The pipeline declares [ses, sub] and, with the alphabetical fallback,
    // used to put `ses` first in the contexts and sort by it.
    let files = [
        "data/sub-1/ses-1/image.nii.gz",
        "data/sub-1/ses-2/image.nii.gz",
        "data/sub-2/ses-1/image.nii.gz",
        "data/sub-10/ses-1/image.nii.gz",
    ];
    let tree = Tree::new("context-order", &files);
    let pipeline = parse_pipeline("source image: Image [ses, sub]\n").unwrap();
    let spec = parse_input_spec(RECIPE).unwrap();
    let resolved = spec
        .resolve(&pipeline, InputSource::Discover(tree.path()))
        .unwrap();
    let text = spit::render_source_inventory(&resolved.inventory, &pipeline, &spec.rules);
    let contexts: Vec<_> = text
        .lines()
        .skip_while(|line| *line != "contexts sessions:")
        .skip(1)
        .filter(|line| line.starts_with("    ["))
        .collect();
    assert_eq!(
        contexts,
        [
            "    [sub=1,ses=1]:",
            "    [sub=1,ses=2]:",
            "    [sub=2,ses=1]:",
            "    [sub=10,ses=1]:",
        ],
        "{text}"
    );
    // Records keep their own product's declared order.
    assert!(text.contains("        image\n"), "{text}");
}

#[test]
fn a_recipe_that_does_not_fit_its_pipeline_says_why() {
    let pipeline = spit::parse_pipeline(
        "source raw [sub]\nsource other [sub]\npath other: o/{sub}.txt\n\
         operation f(raw) -> Out\nout = f(raw)\n",
    )
    .unwrap();
    for (recipe, error, message) in [
        (
            "path out: x/{sub}.txt\n",
            spit::InputError::NotASource {
                product: "out".into(),
            },
            "input path `out` must name a source product in the pipeline",
        ),
        (
            "path other: y/{sub}.txt\n",
            spit::InputError::PathInBoth {
                product: "other".into(),
            },
            "source `other` has path rules in both .spit and .spitin",
        ),
        (
            "discover subs: [sub] from dirs data/sub-{sub}\n",
            spit::InputError::NoDiscoveryPath {
                product: "raw".into(),
            },
            "source `raw` needs a path rule in .spitin for directory discovery",
        ),
    ] {
        let found = spit::parse_input_spec(recipe)
            .unwrap()
            .check(&pipeline)
            .unwrap_err();
        assert_eq!(found, error);
        assert_eq!(found.to_string(), message);
    }
}
