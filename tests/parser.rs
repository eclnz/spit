use spit::{
    parse_document, parse_pipeline, parse_source_inventory, render_dag, resolve, ParseError,
    ResolveError, TypeExpr,
};

const EXAMPLE: &str = include_str!("../examples/basic/basic.spit");
const INVENTORY: &str = include_str!("../examples/basic/basic.sources");

#[test]
fn parses_and_resolves_user_facing_example() {
    let (pipeline, embedded_inventory) = parse_document(EXAMPLE).unwrap();
    assert_eq!(pipeline.products.len(), 5);
    assert_eq!(pipeline.operations.len(), 3);
    assert_eq!(pipeline.invocations.len(), 3);
    assert_eq!(pipeline.constraints.len(), 2);
    let inventory = embedded_inventory.unwrap();
    assert_eq!(inventory, parse_source_inventory(INVENTORY).unwrap());
    assert_eq!(inventory.artifacts.len(), 3);
    let dag = resolve(&pipeline, &inventory).unwrap();
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
    let text = "sources:\n  bold[sub=01,sub=02]\n";
    assert!(matches!(
        parse_source_inventory(text),
        Err(ParseError { line: 2, .. })
    ));
}

#[test]
fn rejects_source_inventory_inside_pipeline_file() {
    let text = "products:\n  bold : BOLD [sub]\nsources:\n  bold[sub=01]\n";
    assert!(matches!(
        parse_pipeline(text),
        Err(ParseError { line: 3, .. })
    ));
}

#[test]
fn reports_semantic_type_error_after_parsing() {
    let text = EXAMPLE.replace(
        "registered = register(denoised, t1w)",
        "registered = register(denoised, bold)",
    );
    let (pipeline, inventory) = parse_document(&text).unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory.unwrap()),
        Err(ResolveError::TypeMismatch { .. })
    ));
}

#[test]
fn resolves_untyped_pipeline_by_shape_and_cardinality() {
    let (pipeline, inventory) = parse_document(include_str!("../examples/types/untyped.spit")).unwrap();
    assert!(pipeline
        .products
        .iter()
        .all(|product| product.artifact_type == TypeExpr::Unknown));
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    assert_eq!(dag.jobs.len(), 3);
    assert_eq!(dag.jobs[2].inputs.len(), 2);
    assert_eq!(dag.jobs[2].output.artifact_type, TypeExpr::Unknown);
    assert!(!dag.jobs[2].output.entities.0.contains_key("repeat"));
    assert!(!render_dag(&dag).contains(": Unknown"));
}

#[test]
fn partially_typed_pipeline_accepts_unknown_and_rejects_known_mismatch() {
    let text = "products:\n  raw [sub]\n  output : Result [sub]\noperations:\n  process(Input) -> Result\npipeline:\n  output = process(raw)\nsources:\n  raw[sub=01]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    assert_eq!(
        resolve(&pipeline, &inventory.unwrap()).unwrap().jobs.len(),
        1
    );

    let mismatched = text.replace("raw [sub]", "raw : Other [sub]");
    let (pipeline, inventory) = parse_document(&mismatched).unwrap();
    assert!(matches!(
        resolve(&pipeline, &inventory.unwrap()),
        Err(ResolveError::TypeMismatch { .. })
    ));
}

#[test]
fn separate_pipeline_still_parses_without_inventory() {
    let (pipeline, inventory) = parse_document(include_str!("../examples/types/typed.spit")).unwrap();
    assert!(inventory.is_none());
    assert!(!pipeline.products.is_empty());
}

#[test]
fn flow_form_infers_intermediate_products_and_keeps_inventory_separate() {
    let text = "source raw : Image<Native> [subject, run]\n\
operation clean(Image<S>) -> Clean<S>\n\
cleaned = clean(raw)\n\
operation mean(many Clean<S>) -> Mean<S>\n\
average : Mean<Native> [subject] = mean(cleaned @ vary(run))\n\
require raw count>=1 per [subject]\n\
sources:\n\
    raw[subject=A,run=1]\n\
    raw[subject=A,run=2]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let inventory = inventory.unwrap();
    assert_eq!(pipeline.products.len(), 3);
    assert_eq!(pipeline.operations.len(), 2);
    assert_eq!(pipeline.invocations.len(), 2);
    assert_eq!(pipeline.constraints.len(), 1);
    assert_eq!(pipeline.products[1].name, "cleaned");
    assert_eq!(pipeline.products[1].dimensions, vec!["subject", "run"]);
    assert_eq!(pipeline.products[2].dimensions, vec!["subject"]);
    assert_eq!(resolve(&pipeline, &inventory).unwrap().jobs.len(), 3);
}

#[test]
fn flow_form_requires_operation_declaration_before_use() {
    let error = parse_pipeline("source raw [subject]\nresult = transform(raw)\n").unwrap_err();
    assert!(error
        .to_string()
        .contains("must be declared before its first flow step"));
}

#[test]
fn command_arguments_keep_quoted_hashes_and_strip_comments() {
    let text = "source raw [id]# source comment\noperation copy(one)\ncommand copy: tool --tag '#run' --label \"part#1\" {input} {output}# command comment\nresult = copy(raw)\n";
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(
        pipeline.commands[0].template,
        "tool --tag '#run' --label \"part#1\" {input} {output}"
    );
}

#[test]
fn equals_command_keeps_colons_in_arguments() {
    let pipeline = parse_pipeline(
        "source raw [id]\noperation fetch(one)\ncommand fetch = tool --url https://example.com/a:b {input} {output}\nresult = fetch(raw)\n",
    )
    .unwrap();
    assert_eq!(pipeline.commands[0].operation, "fetch");
    assert_eq!(
        pipeline.commands[0].template,
        "tool --url https://example.com/a:b {input} {output}"
    );
}

#[test]
fn named_ports_and_declared_aggregate_shape_are_checked() {
    let text = "source raw [sub, run]\noperation combine(runs: many) @ drop(run)\nresult = combine(raw @ vary(run))\nsources:\n  raw[sub=01,run=2]\n  raw[sub=01,run=1]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    assert_eq!(pipeline.operations[0].inputs[0].name, "runs");
    assert_eq!(
        pipeline.operations[0].aggregated_dimension.as_deref(),
        Some("run")
    );
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    assert_eq!(dag.jobs[0].output.entities.0.len(), 1);

    let wrong_vary = text.replace("vary(run)", "vary(sub)");
    let (pipeline, inventory) = parse_document(&wrong_vary).unwrap();
    assert!(resolve(&pipeline, &inventory.unwrap())
        .unwrap_err()
        .to_string()
        .contains("declares drop(run) but invocation uses vary(sub)"));

    let wrong_shape = text.replace("result =", "result : Data [sub, run] =");
    let (pipeline, inventory) = parse_document(&wrong_shape).unwrap();
    assert!(resolve(&pipeline, &inventory.unwrap()).is_err());
}

#[test]
fn shell_source_is_rejected_with_migration_guidance() {
    let text = "source raw [id]\noperation copy(one)\nresult = copy(raw)\nsources:\n  raw[id=x]\nshell-source: scripts/functions.sh\n";
    let error = parse_document(text).unwrap_err();
    assert_eq!(error.line, 6);
    assert!(error.message.contains("executable available on PATH"));

    let error = parse_pipeline("products:\n  raw [id]\nshell-source: scripts/functions.sh\n")
        .unwrap_err();
    assert_eq!(error.line, 3);
    assert!(error.message.contains("executable available on PATH"));
}

#[test]
fn rejects_unbalanced_command_brackets_with_line_number() {
    let cases = [
        ("command normalize: normalize --mode input} {output", "unmatched `}`"),
        ("command normalize: normalize --mode {input {output}", "unclosed `{`"),
        ("command normalize: normalize --mode {input} {{output}", "unmatched `}`"),
        ("command normalize: normalize --mode {} {output}", "empty placeholder"),
        ("command normalize: normalize '--mode {input} {output}", "unterminated quote"),
    ];
    for (line, expected) in cases {
        let text = format!("source raw : Table [id]\n{line}\n");
        let error = parse_pipeline(&text).unwrap_err();
        assert_eq!(error.line, 2, "{line}");
        assert!(error.message.contains("normalize"), "{error}");
        assert!(error.message.contains(expected), "{line}: {error}");
    }
}

#[test]
fn path_template_errors_are_reported_while_parsing() {
    let cases = [
        ("path: {product}/{entities.csv", "unclosed `{`"),
        ("path raw: raw/id}.csv", "unmatched `}`"),
    ];
    for (line, expected) in cases {
        let text = format!("source raw : Table [id]\n{line}\n");
        let error = parse_pipeline(&text).unwrap_err();
        assert_eq!(error.line, 2, "{line}");
        assert!(error.message.contains(expected), "{line}: {error}");
    }
}

#[test]
fn hash_joined_to_text_is_rejected_instead_of_truncating() {
    let text = "source raw [id]\noperation copy(one)\ncommand copy: tool --color=#fff {input} {output}\n";
    let error = parse_pipeline(text).unwrap_err();
    assert_eq!(error.line, 3);
    assert!(error.message.contains("`--color=`"), "{error}");
    assert!(parse_pipeline(
        "source raw [id]\noperation copy(one)\ncommand copy: tool '--color=#fff' {input} {output} # note\n"
    )
    .is_ok());
}
