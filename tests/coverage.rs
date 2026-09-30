//! `require` rules: the input stage checks each group of an inventory
//! against them before any job is resolved.

mod support;

use std::path::Path;

use spit::{
    diagnose_in, parse_source_inventory, resolve, CountRequirement, CoverageRule, EntityBinding,
    InputRules, InputSource, InputSpec, Pipeline, ProductDef, ResolveError, ResolvedDag,
    SourceInventory, SourceRecord, TypeExpr,
};

/// Settle `inventory` with the rules written in `text`, then resolve jobs.
fn resolve_text(text: &str, inventory: &str) -> Result<ResolvedDag, ResolveError> {
    let (pipeline, spec, _) = support::parse_with_rules(text).unwrap();
    let settled = spec
        .resolve(
            &pipeline,
            InputSource::Inventory(parse_source_inventory(inventory).unwrap()),
        )
        .unwrap();
    settled.require_complete()?;
    resolve(&pipeline, &settled.dag_inventory())
}

fn record(product: &str, pairs: &[(&str, &str)]) -> SourceRecord {
    SourceRecord::new(
        product,
        pairs
            .iter()
            .map(|(dimension, value)| ((*dimension).to_owned(), (*value).to_owned()))
            .collect::<EntityBinding>(),
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
    // The rule is in the recipe, so the error names no pipeline line.
    let (pipeline, recipe) = support::split_rules(text);
    let spec = spit::parse_input_spec(&recipe).unwrap();
    let context = spit::Context {
        recipe: Some(&spec),
        ..spit::Context::at(Path::new("a.spit"))
    };
    let issues: Vec<_> = diagnose_in(&pipeline, Some(incomplete), context)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        issues,
        ["error: source coverage for `image` at [subject=a]: no artifact with run=2"]
    );
}

#[test]
fn a_rule_is_checked_against_the_pipeline_without_an_inventory() {
    let text = "source image [subject, run]\nrequire image subject=a per [subject]\n";
    let (pipeline, spec, _) = support::parse_with_rules(text).unwrap();
    let error = spec.check(&pipeline).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("`subject` is one of the rule's groups ([subject])"),
        "{error}"
    );
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
        ..InputSpec::default()
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
