mod support;

use spit::{parse_input_spec, parse_pipeline, parse_source_inventory, resolve};

/// A sectioned pipeline with the same shape as the basic example.
const PIPELINE: &str = "\
products:
    signal      : Signal         [site, day, run]
    calibration : Calibration    [site, day]
    denoised    : FilteredSignal [site, day, run]
    registered  : AlignedSignal  [site, day, run]
    mean_signal : MeanSignal     [site, day]

operations:
    denoise(Signal) -> FilteredSignal
    register(FilteredSignal, Calibration) -> AlignedSignal
    mean(many AlignedSignal) -> MeanSignal

pipeline:
    denoised = denoise(signal)
    registered = register(denoised, calibration)
    mean_signal = mean(registered @ vary(run))

";
const RECIPE: &str = "\
require signal count>=1 per [site, day]
require calibration count=1 per [site, day]
";
const INVENTORY: &str = "\
contexts:
    [site=01,day=01]

sources:
    signal[site=01,day=01,run=1]
    signal[site=01,day=01,run=2]
    calibration[site=01,day=01]
";

#[test]
fn parses_a_sectioned_pipeline_its_recipe_and_its_records() {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    assert_eq!(pipeline.products.len(), 5);
    assert_eq!(pipeline.operations.len(), 3);
    assert_eq!(pipeline.invocations.len(), 3);
    assert_eq!(parse_input_spec(RECIPE).unwrap().rules.constraints.len(), 2);
    assert_eq!(
        parse_source_inventory(INVENTORY).unwrap().artifacts.len(),
        3
    );
}

#[test]
fn rules_and_records_belong_outside_the_pipeline() {
    for (text, line, file) in [
        ("source x [a]\nrequire x count>=1 per [a]\n", 2, ".spitin"),
        ("source x [a]\nskip x count>=1 per [a]\n", 2, ".spitin"),
        (
            "discover s: [a] from dirs d/{a}\nsource x [a]\n",
            1,
            ".spitin",
        ),
        ("source x [a]\nsources:\n    x[a=1]\n", 2, ".spitout"),
        ("source x [a]\ncontexts:\n    [a=1]\n", 2, ".spitout"),
        (
            "products:\n    x : X [a]\nconstraints:\n    require x count>=1 per [a]\n",
            4,
            ".spitin",
        ),
    ] {
        let error = parse_pipeline(text).unwrap_err();
        assert_eq!(error.line(), line, "{text}");
        assert!(error.to_string().contains(file), "{text}: {error}");
    }
}

#[test]
fn reports_line_for_bad_text() {
    let text = "products:\n  signal : Signal [site, run]\npipeline:\n  denoised = denoise(signal @ vary(run, day))\n";
    let error = parse_pipeline(text).unwrap_err();
    assert_eq!(error.line(), 4);
    assert!(error.to_string().contains("line 4"));
}

#[test]
fn rejects_duplicate_dimension_in_source() {
    let text = "sources:\n  signal[site=01,site=02]\n";
    assert_eq!(parse_source_inventory(text).unwrap_err().line(), 2);
}

#[test]
fn rejects_source_inventory_inside_pipeline_file() {
    let text = "products:\n  signal : Signal [site]\nsources:\n  signal[site=01]\n";
    assert_eq!(parse_pipeline(text).unwrap_err().line(), 3);
}

#[test]
fn separate_pipeline_still_parses_without_inventory() {
    let (pipeline, inventory) =
        support::parse_fixture(include_str!("../examples/types/typed.spit")).unwrap();
    assert!(inventory.is_none());
    assert!(!pipeline.products.is_empty());
}

#[test]
fn flow_form_infers_intermediate_products_and_keeps_inventory_separate() {
    let text = "source raw : Image<Native> [site, run]\n\
operation clean(Image<S>) -> Clean<S>\n\
cleaned = clean(raw)\n\
operation mean(many Clean<S>) -> Mean<S>\n\
average : Mean<Native> [site] = mean(cleaned @ vary(run))\n\
sources:\n\
    raw[site=A,run=1]\n\
    raw[site=A,run=2]\n";
    let (pipeline, inventory) = support::parse_fixture(text).unwrap();
    let (pipeline, inventory) = (&pipeline, inventory.unwrap());
    assert_eq!(pipeline.products.len(), 3);
    assert_eq!(pipeline.operations.len(), 2);
    assert_eq!(pipeline.invocations.len(), 2);
    assert_eq!(pipeline.products[1].name, "cleaned");
    assert_eq!(pipeline.products[1].dimensions, vec!["site", "run"]);
    assert_eq!(pipeline.products[2].dimensions, vec!["site"]);
    assert_eq!(resolve(pipeline, &inventory).unwrap().jobs.len(), 3);
}

#[test]
fn flow_form_requires_operation_declaration_before_use() {
    let error = parse_pipeline("source raw [site]\nresult = transform(raw)\n").unwrap_err();
    assert!(error
        .to_string()
        .contains("must be declared before its first flow step"));
}

#[test]
fn command_arguments_keep_quoted_hashes_and_strip_comments() {
    let text = "source raw [id] # source comment\n# whole-line comment\noperation copy(one)\ncommand copy: tool --tag '#run' --label \"part#1\" --color=#fff {input} {output} # command comment\nresult = copy(raw)\n";
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(
        pipeline.commands[0].template,
        "tool --tag '#run' --label \"part#1\" --color=#fff {input} {output}"
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
fn shell_source_is_rejected_with_migration_guidance() {
    let text = "source raw [id]\noperation copy(one)\nresult = copy(raw)\nshell-source: scripts/functions.sh\n";
    let error = parse_pipeline(text).unwrap_err();
    assert_eq!(error.line(), 4);
    assert!(error.message.contains("executable available on PATH"));

    let error =
        parse_pipeline("products:\n  raw [id]\nshell-source: scripts/functions.sh\n").unwrap_err();
    assert_eq!(error.line(), 3);
    assert!(error.message.contains("executable available on PATH"));
}

#[test]
fn rejects_unbalanced_command_brackets_with_line_number() {
    let cases = [
        (
            "command normalize: normalize --mode input} {output",
            "unmatched `}`",
        ),
        (
            "command normalize: normalize --mode {input {output}",
            "unclosed `{`",
        ),
        (
            "command normalize: normalize --mode {input} {{output}",
            "unmatched `}`",
        ),
        (
            "command normalize: normalize --mode {} {output}",
            "empty placeholder",
        ),
        (
            "command normalize: normalize '--mode {input} {output}",
            "unterminated quote",
        ),
    ];
    for (line, expected) in cases {
        let text = format!("source raw : Table [id]\n{line}\n");
        let error = parse_pipeline(&text).unwrap_err();
        assert_eq!(error.line(), 2, "{line}");
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
        assert_eq!(error.line(), 2, "{line}");
        assert!(error.message.contains(expected), "{line}: {error}");
    }
}

#[test]
fn hash_inside_a_word_is_text_as_in_bash() {
    let pipeline = parse_pipeline(
        "source raw [id]\noperation copy(one)\ncommand copy: tool --url=https://example.com/#top {input} {output}# note\n",
    )
    .unwrap();
    assert_eq!(
        pipeline.commands[0].template,
        "tool --url=https://example.com/#top {input} {output}# note"
    );
    let error = parse_pipeline("source raw [id]# note\n").unwrap_err();
    assert_eq!(error.line(), 1);
}

#[test]
fn input_port_cannot_shadow_output_placeholder() {
    let error =
        parse_pipeline("source raw [id]\noperation copy(output: Image) -> Image\n").unwrap_err();
    assert_eq!(error.line(), 2);
    assert!(error.message.contains("`output` is reserved"));
}

#[test]
fn commands_header_alone_selects_sectioned_form() {
    let pipeline =
        parse_pipeline("products:\n  raw [id]\ncommands:\n  copy: tool {input} {output}\n")
            .unwrap();
    assert_eq!(pipeline.commands.len(), 1);
    let pipeline = parse_pipeline("commands:\n  copy: tool {input} {output}\n").unwrap();
    assert_eq!(pipeline.commands[0].operation, "copy");
}

#[test]
fn a_source_record_may_give_its_file() {
    let text = "sources:\n    raw[site=A]: data/A/raw.txt\n    raw[site=B]\n";
    let inventory = parse_source_inventory(text).unwrap();
    assert_eq!(
        inventory.artifacts[0].path.as_deref(),
        Some("data/A/raw.txt")
    );
    assert_eq!(inventory.artifacts[1].path, None);
    let pipeline = parse_pipeline("source raw [site]\n").unwrap();
    let rendered = spit::render_source_inventory(&inventory, &pipeline);
    assert_eq!(parse_source_inventory(&rendered).unwrap(), inventory);
    for bad in [
        "raw[site=A] data.txt",
        "raw[site=A]:",
        "raw[site=A]: /abs.txt",
    ] {
        let error = parse_source_inventory(&format!("sources:\n{bad}\n")).unwrap_err();
        assert_eq!(error.line(), 2, "{bad}");
    }
}
