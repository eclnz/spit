//! Coverage rules that require entity values (`require image run=1,2 per [subject]`).

use spit::{diagnose, parse_pipeline, parse_source_inventory, resolve, ResolveError, ResolvedDag};

fn resolve_text(text: &str, inventory: &str) -> Result<ResolvedDag, ResolveError> {
    let pipeline = parse_pipeline(text).unwrap();
    resolve(&pipeline, &parse_source_inventory(inventory).unwrap())
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
    let grouped = text.replace("run=1,2 per [subject]", "subject=a per [subject]");
    let error = spit::validate_pipeline(&parse_pipeline(&grouped).unwrap()).unwrap_err();
    assert!(error.to_string().contains("outside its groups"), "{error}");
}
