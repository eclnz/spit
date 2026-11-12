//! Path rules: which rule covers each product, and rules that cannot tell
//! artifacts apart.

mod support;

use support::bound;

use spit::{
    inspect_paths, parse_pipeline, parse_source_inventory, resolve, PathRule, PathTemplate,
};

#[test]
fn path_coverage_exposes_default_fallbacks_and_accepts_them() {
    let (pipeline, _) = support::parse_fixture(include_str!(
        "../examples/commands/field_survey/field_survey.spit"
    ))
    .unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert!(coverage.entries.iter().any(|entry| {
        entry.product == "vegetation" && matches!(entry.rule, PathRule::Default(_))
    }));
    assert!(coverage.entries.iter().any(|entry| {
        entry.product == "raw_photo" && matches!(entry.rule, PathRule::Explicit(_))
    }));
    coverage.validate(["vegetation", "raw_photo"]).unwrap();
}

#[test]
fn an_output_with_no_rule_is_an_error_and_a_source_waits_for_one() {
    let pipeline =
        parse_pipeline("source raw [id]\noperation clean(x: Raw) -> Clean\ncleaned = clean(raw)\n")
            .unwrap();
    assert!(inspect_paths(&pipeline)
        .unwrap_err()
        .to_string()
        .contains("output `cleaned` has no path rule"));
    let pipeline = parse_pipeline(
        "source raw [id]\noperation clean(x: Raw) -> Clean\ncleaned = clean(raw)\npath cleaned: out/{id}.txt\n",
    )
    .unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert_eq!(coverage.entries[0].rule, PathRule::Missing);
    coverage.validate(["cleaned"]).unwrap();
    assert!(coverage
        .validate(["raw", "cleaned"])
        .unwrap_err()
        .to_string()
        .contains("source `raw` has no path rule"));
}

#[test]
fn path_coverage_catches_missing_and_invalid_rules_without_jobs() {
    let mut pipeline = parse_pipeline("source unused [id]\n").unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert_eq!(coverage.entries[0].rule, PathRule::Missing);
    assert!(coverage.validate(["unused"]).is_err());
    coverage.validate([]).unwrap();

    pipeline.product_paths.insert(
        "unused".to_owned(),
        PathTemplate::parse("input/{id}/{missing}.txt").unwrap(),
    );
    assert!(inspect_paths(&pipeline)
        .unwrap_err()
        .to_string()
        .contains("absent dimension"));

    pipeline.product_paths.insert(
        "unused".to_owned(),
        PathTemplate::parse("input/{id}.txt").unwrap(),
    );
    inspect_paths(&pipeline)
        .unwrap()
        .validate(["unused"])
        .unwrap();
}

#[test]
fn bound_dag_shows_port_names_and_paths_without_commands() {
    let (mut pipeline, _) = support::parse_fixture(include_str!(
        "../examples/commands/field_survey/field_survey.spit"
    ))
    .unwrap();
    let inventory = parse_source_inventory(include_str!(
        "../examples/commands/field_survey/field_survey.spitout"
    ))
    .unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    pipeline.commands.clear();
    let report = bound(&pipeline, &dag).unwrap();
    assert_eq!(report.matches("Job ").count(), 93);
    assert!(report.contains("moving: ground_map[site=01,visit=01]"));
    assert!(report.contains("reference: visit_dark_tiff[site=01,visit=01]"));
    assert!(report.contains("path: derivatives/yield_table/site=01__visit=01.csv"));

    // Without the pipeline's `default path:`, an output no rule covers has
    // no path, which is an error rather than a hidden default.
    pipeline.path_template = None;
    assert_eq!(
        bound(&pipeline, &dag).unwrap_err(),
        "output `photo_img` has no path rule; write `path photo_img: ...`, or a `default path:` for the pipeline or its stage"
    );
}

#[test]
fn path_rules_that_cannot_separate_artifacts_are_rejected() {
    let check = |text: &str| inspect_paths(&parse_pipeline(text).unwrap()).map(|_| ());

    let error =
        check("source raw [id, batch]\ndefault path: {@product}/{@entities}.csv\npath raw: raw/{id}.csv\ndefault entities: {@key}={@value} separated \"__\"\n")
            .unwrap_err();
    assert!(
        error.message().contains("omits dimension `batch`"),
        "{error}"
    );

    let error = check(
        "source raw [id]\ndefault path: {@entities}.csv\noperation clean(input)\ncleaned = clean(raw)\ndefault entities: {@key}={@value} separated \"__\"\n",
    )
    .unwrap_err();
    assert!(error.message().contains("`raw` and `cleaned`"), "{error}");

    let error = check("source raw [id]\npath raw: {@product}/{id}/{shard}.csv\n").unwrap_err();
    assert_eq!(
        error.to_string(),
        "path template for `raw` uses absent dimension `shard`; put it in `[...]` if only some products have it"
    );

    // A default shared with other products can also be left for a rule of
    // the product's own.
    let error = check(
        "source raw [id]\ndefault path: {@product}/{id}.csv\noperation total(raw: many) -> Table\ncommand total: tool {raw} {@output}\nsummary = total(raw @vary(id))\n",
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "path template for `summary` uses absent dimension `id`; put it in `[...]` if only some products have it, or give `summary` a rule of its own with `path summary:`"
    );

    // Rules naming different dimensions are not treated as colliding.
    check("source raw [id]\nsource extra [batch]\npath raw: out/{id}.csv\npath extra: out/{batch}.csv\n")
        .unwrap();
}

#[test]
fn a_default_naming_a_dimension_a_source_lacks_leaves_it_to_the_recipe() {
    let tree = support::Tree::new(
        "default-absent-dimension",
        &["data/in/01/a.txt", "data/scripts/plot.py"],
    );
    tree.write(
        "b.spit",
        "source raw : Text .txt [sub]\nsource plotter : Script .py\ndefault path: out/sub-{sub}/{@product}\noperation plot(raw: Text, script: Script) -> Figure .png\ncommand plot: python {script} {raw} {@output}\nfig = plot(raw, plotter)\n",
    );
    let recipe = |rules: &str| {
        format!("pipeline b.spit\nroot data\ninputs:\n    raw: in/{{sub}}/a.txt\n{rules}")
    };
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .current_dir(tree.path())
            .output()
            .unwrap()
    };

    tree.write("b.spitin", &recipe("    plotter: scripts/plot.py\n"));
    let output = run(&["check", "b.spitin"]);
    assert!(output.status.success(), "{}", support::text(&output.stderr));

    // Without the recipe's rule, the source has none, and the error says
    // where to write one.
    tree.write("b.spitin", &recipe(""));
    let output = run(&["check", "b.spitin"]);
    assert!(
        support::text(&output.stderr).contains("source `plotter` has no path rule"),
        "{}",
        support::text(&output.stderr)
    );
}
