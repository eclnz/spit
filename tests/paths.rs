//! Path rules: which rule covers each product, and rules that cannot tell
//! artifacts apart.

mod support;

use support::bound;

use spit::{
    inspect_paths, parse_pipeline, parse_source_inventory, resolve, PathRule, PathTemplate,
};

#[test]
fn path_coverage_exposes_default_fallbacks_and_strict_rejects_them() {
    let (pipeline, _) = support::parse_fixture(include_str!(
        "../examples/commands/field_survey/field_survey.spit"
    ))
    .unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert!(coverage.entries.iter().any(|entry| {
        entry.product == "vegetation" && matches!(entry.rule, PathRule::Default(_))
    }));
    assert!(coverage.entries.iter().any(|entry| {
        entry.product == "photo_response" && matches!(entry.rule, PathRule::Explicit(_))
    }));
    coverage.validate(false).unwrap();
    assert!(coverage
        .validate(true)
        .unwrap_err()
        .to_string()
        .contains("vegetation"));
}

#[test]
fn path_coverage_catches_missing_and_invalid_rules_without_jobs() {
    let mut pipeline = parse_pipeline("source unused [id]\n").unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    assert_eq!(coverage.entries[0].rule, PathRule::Missing);
    assert!(coverage.validate(false).is_err());

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
    inspect_paths(&pipeline).unwrap().validate(true).unwrap();
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

    pipeline.product_paths.remove("yield_table");
    pipeline.path_template = None;
    assert!(bound(&pipeline, &dag)
        .unwrap_err()
        .to_string()
        .contains("no path rule"));
}

#[test]
fn path_rules_that_cannot_separate_artifacts_are_rejected() {
    let check = |text: &str| inspect_paths(&parse_pipeline(text).unwrap()).map(|_| ());

    let error =
        check("source raw [id, batch]\npath: {product}/{entities}.csv\npath raw: raw/{id}.csv\n")
            .unwrap_err();
    assert!(
        error.message().contains("omits dimension `batch`"),
        "{error}"
    );

    let error = check(
        "source raw [id]\npath: {entities}.csv\noperation clean(input)\ncleaned = clean(raw)\n",
    )
    .unwrap_err();
    assert!(error.message().contains("`raw` and `cleaned`"), "{error}");

    let error = check("source raw [id]\npath: {product}/{id}/{shard}.csv\n").unwrap_err();
    assert_eq!(
        error.to_string(),
        "path template for `raw` uses absent dimension `shard`"
    );

    // Rules naming different dimensions are not treated as colliding.
    check("source raw [id]\nsource extra [batch]\npath raw: out/{id}.csv\npath extra: out/{batch}.csv\n")
        .unwrap();
}
