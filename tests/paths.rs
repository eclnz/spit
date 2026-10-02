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
fn an_output_with_no_rule_takes_the_built_in_default_and_a_source_none() {
    let pipeline =
        parse_pipeline("source raw [id]\noperation clean(x: Raw) -> Clean\ncleaned = clean(raw)\n")
            .unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert_eq!(coverage.entries[0].rule, PathRule::Missing);
    assert_eq!(
        coverage.entries[1].rule,
        PathRule::BuiltIn("out/{@product}/{@entities}".to_owned())
    );
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

    // An output no rule covers takes the built-in default.
    pipeline.product_paths.remove("yield_table");
    pipeline.path_template = None;
    assert!(bound(&pipeline, &dag)
        .unwrap()
        .contains("path: out/yield_table/site=01__visit=01.csv"));
}

#[test]
fn path_rules_that_cannot_separate_artifacts_are_rejected() {
    let check = |text: &str| inspect_paths(&parse_pipeline(text).unwrap()).map(|_| ());

    let error =
        check("source raw [id, batch]\npath: {@product}/{@entities}.csv\npath raw: raw/{id}.csv\n")
            .unwrap_err();
    assert!(
        error.message().contains("omits dimension `batch`"),
        "{error}"
    );

    let error = check(
        "source raw [id]\npath: {@entities}.csv\noperation clean(input)\ncleaned = clean(raw)\n",
    )
    .unwrap_err();
    assert!(error.message().contains("`raw` and `cleaned`"), "{error}");

    let error = check("source raw [id]\npath: {@product}/{id}/{shard}.csv\n").unwrap_err();
    assert_eq!(
        error.to_string(),
        "path template for `raw` uses absent dimension `shard`; put it in `[...]` if only some products have it"
    );

    // Rules naming different dimensions are not treated as colliding.
    check("source raw [id]\nsource extra [batch]\npath raw: out/{id}.csv\npath extra: out/{batch}.csv\n")
        .unwrap();
}
