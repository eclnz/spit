use spit::{parse_pipeline, resolve, ParseError, ResolveError};

const EXAMPLE: &str = include_str!("../examples/basic.spit");

#[test]
fn parses_and_resolves_user_facing_example() {
    let pipeline = parse_pipeline(EXAMPLE).unwrap();
    assert_eq!(pipeline.products.len(), 5);
    assert_eq!(pipeline.operations.len(), 3);
    assert_eq!(pipeline.invocations.len(), 3);
    assert_eq!(pipeline.sources.len(), 3);
    let dag = resolve(&pipeline).unwrap();
    assert_eq!(dag.jobs.len(), 5);
    assert_eq!(dag.jobs[4].inputs.len(), 2);
    assert!(!dag.jobs[4].output.entities.0.contains_key("run"));
}

#[test]
fn reports_line_for_bad_text() {
    let text = "products:\n  bold : BOLD [sub, run]\npipeline:\n  denoised = denoise(bold @ vary(run, ses))\n";
    let error = parse_pipeline(text).unwrap_err();
    assert_eq!(error.line, 4);
    assert!(error.to_string().contains("line 4"));
}

#[test]
fn rejects_duplicate_dimension_in_source() {
    let text = "products:\n  bold : BOLD [sub]\nsources:\n  bold[sub=01,sub=02]\n";
    assert!(matches!(
        parse_pipeline(text),
        Err(ParseError { line: 4, .. })
    ));
}

#[test]
fn reports_semantic_type_error_after_parsing() {
    let text = EXAMPLE.replace(
        "registered = register(denoised, t1w)",
        "registered = register(denoised, bold)",
    );
    let pipeline = parse_pipeline(&text).unwrap();
    assert!(matches!(
        resolve(&pipeline),
        Err(ResolveError::TypeMismatch { .. })
    ));
}
