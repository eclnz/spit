//! `require` rules: the input stage checks each group of an inventory
//! against them before any job is resolved.

use spit::{
    diagnose, parse_source_inventory, parse_spit, resolve, CountRequirement, CoverageRule,
    EntityBinding, InputRules, InputSource, InputSpec, Pipeline, ProductDef, ResolveError,
    ResolvedDag, SourceInventory, SourceRecord, TypeExpr,
};

/// Settle `inventory` with the rules written in `text`, then resolve jobs.
fn resolve_text(text: &str, inventory: &str) -> Result<ResolvedDag, ResolveError> {
    let document = parse_spit(text).unwrap();
    let settled = InputSpec::embedded_in(&document)
        .resolve(
            &document.pipeline,
            InputSource::Inventory(parse_source_inventory(inventory).unwrap()),
        )
        .unwrap();
    settled.require_complete()?;
    resolve(&document.pipeline, &settled.dag_inventory())
}

fn record(product: &str, pairs: &[(&str, &str)]) -> SourceRecord {
    SourceRecord::new(
        product,
        EntityBinding(
            pairs
                .iter()
                .map(|(dimension, value)| ((*dimension).to_owned(), (*value).to_owned()))
                .collect(),
        ),
    )
}

#[test]
fn coverage_rules_can_require_entity_values() {
    let text = "source image [subject, run]\nrequire image run=1,2 per [subject]\noperation f(Image) -> Image\nout = f(image)\n";
    let complete = "sources:\n  image[subject=a,run=1]\n  image[subject=a,run=2]\n";
    assert!(resolve_text(text, complete).is_ok());
    let incomplete = "sources:\n  image[subject=a,run=1]\n  image[subject=a,run=3]\n";
    assert!(matches!(
        resolve_text(text, incomplete),
        Err(ResolveError::MissingRequiredValue { dimension, value, .. })
            if dimension == "run" && value == "2"
    ));
    let issues: Vec<_> = diagnose(text, Some(incomplete))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        issues,
        ["error: line 2: source coverage for `image` at [subject=a]: no artifact with run=2"]
    );
}

#[test]
fn a_rule_is_checked_against_the_pipeline_without_an_inventory() {
    let text = "source image [subject, run]\nrequire image subject=a per [subject]\n";
    let document = parse_spit(text).unwrap();
    let error = InputSpec::embedded_in(&document)
        .check(&document.pipeline)
        .unwrap_err();
    assert!(error.to_string().contains("outside its groups"), "{error}");
}

#[test]
fn coverage_checks_each_observed_context_without_a_global_count() {
    let pipeline = Pipeline {
        products: vec![ProductDef::new(
            "image",
            TypeExpr::named("Image"),
            ["site", "visit"],
        )],
        ..Pipeline::default()
    };
    let spec = InputSpec {
        rules: InputRules {
            constraints: vec![CoverageRule::new(
                "image",
                ["site", "visit"],
                CountRequirement::Exactly(1),
            )],
            ..InputRules::default()
        },
        inventory: None,
    };
    let settle = |inventory: SourceInventory| {
        spec.resolve(&pipeline, InputSource::Inventory(inventory))
            .unwrap()
            .require_complete()
    };
    let contexts = vec![
        EntityBinding::from_pairs([("site", "A"), ("visit", "1")]),
        EntityBinding::from_pairs([("site", "B"), ("visit", "1")]),
    ];
    let incomplete = SourceInventory {
        artifacts: vec![record("image", &[("site", "A"), ("visit", "1")])],
        contexts: contexts.clone(),
        ..SourceInventory::default()
    };
    assert!(matches!(
        settle(incomplete),
        Err(ResolveError::CoverageViolation { product, found: 0, .. }) if product == "image"
    ));
    let complete = SourceInventory {
        artifacts: vec![
            record("image", &[("site", "A"), ("visit", "1")]),
            record("image", &[("site", "B"), ("visit", "1")]),
        ],
        contexts,
        ..SourceInventory::default()
    };
    assert!(settle(complete).is_ok());
}
