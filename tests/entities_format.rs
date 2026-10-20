//! Custom entities agree across path hints, discovery and saved inventories.

mod support;

use spit::{inspect_paths, parse_pipeline, parse_pipeline_at, parse_source_inventory, resolve};
use support::{bound, spit, text, Tree};

const FORMAT: &str =
    "entities: {key}_{value} separated \"-\" empty \"global\"\nentities sub: subject\n";
const FLOW: &str = "source raw [sub, ses, run]\nsource config\noperation copy(x)\noperation merge(x: many)\ncopied = copy(raw)\nsession = merge(copied @ vary(run))\nsubject = merge(session @ vary(ses))\nall = merge(subject @ vary(sub))\n";

fn ok(args: &[&str]) -> String {
    let result = spit(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        text(&result.stderr)
    );
    text(&result.stdout)
}

#[test]
fn one_format_covers_each_shape_without_changing_identity() {
    let pipeline = parse_pipeline(&format!(
        "{FORMAT}{FLOW}default path: {{@product}}/{{@entities}}.txt\npath config: config.txt\n"
    ))
    .unwrap();
    inspect_paths(&pipeline).unwrap();
    for (product, expected) in [
        ("copied", "{@product}/subject_{sub}-ses_{ses}-run_{run}.txt"),
        ("session", "{@product}/subject_{sub}-ses_{ses}.txt"),
        ("subject", "{@product}/subject_{sub}.txt"),
        ("all", "{@product}/global.txt"),
    ] {
        assert_eq!(
            pipeline.path_template_for(product).unwrap().to_string(),
            expected
        );
    }
    let inventory = parse_source_inventory("sources:\n raw[sub=A_B,ses=01,run=2]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let paths = bound(&pipeline, &dag).unwrap();
    assert!(paths.contains("copied[sub=A_B,ses=01,run=2]"), "{paths}");
    assert!(
        paths.contains("copied/subject_A%5FB-ses_01-run_2.txt"),
        "{paths}"
    );
    assert!(paths.contains("all/global.txt"), "{paths}");
}

#[test]
fn empty_entities_can_drop_an_optional_group_and_output_rules_use_the_format() {
    let pipeline = parse_pipeline(&format!(
        "entities: {{key}}_{{value}} separated \"-\" empty \"\"\n{FLOW}default path: out/[{{@entities}}_]{{@product}}.txt\npath raw: raw/{{@entities}}.txt\npath config: config.txt\n"
    ))
    .unwrap();
    inspect_paths(&pipeline).unwrap();
    assert_eq!(
        pipeline.path_template_for("all").unwrap().to_string(),
        "out/{@product}.txt"
    );
    assert_eq!(
        pipeline.path_template_for("session").unwrap().to_string(),
        "out/sub_{sub}-ses_{ses}_{@product}.txt"
    );
    let pipeline = parse_pipeline(&format!(
        "{FORMAT}{FLOW}path subject: out/{{@product}}/{{@entities}}\n"
    ))
    .unwrap();
    assert_eq!(
        pipeline.path_template_for("subject").unwrap().to_string(),
        "out/{@product}/subject_{sub}"
    );

    let pipeline = parse_pipeline("entities: {key}={value} separated \"__\" empty \"all data\"\nsource config\npath config: in/{@entities}.txt\n").unwrap();
    assert_eq!(
        pipeline.path_template_for("config").unwrap().to_string(),
        "in/all%20data.txt"
    );
}

#[test]
fn fresh_discovery_and_saved_inventory_produce_the_same_dag_and_hints() {
    let tree = Tree::new(
        "entities-discovery",
        &[
            "raw/subject_A%5FB-ses_01-run_1.txt",
            "raw/subject_A%5FB-ses_01-run_2.txt",
            "raw/subject_Z-ses_02-run_1.txt",
        ],
    );
    let pipeline = tree.write(
        "pipeline.spit",
        &format!("{FORMAT}{FLOW}default path: out/{{@product}}/{{@entities}}.txt\n"),
    );
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\npath raw: raw/{@entities}.txt\npath config: config.txt\n",
    );
    tree.write("config.txt", "");
    let saved = tree.path().join("data.spitout");
    let pipeline = pipeline.to_str().unwrap();
    let recipe = recipe.to_str().unwrap();
    let saved = saved.to_str().unwrap();
    ok(&["inputs", recipe, "-o", saved]);
    let inventory = std::fs::read_to_string(saved).unwrap();
    assert!(
        inventory.contains("raw: raw/subject_{sub}-ses_{ses}-run_{run}.txt"),
        "{inventory}"
    );
    assert!(inventory.contains("[sub=A_B,ses=01,run=1]"), "{inventory}");
    let fresh = ok(&["dag", recipe, "--json"]);
    let read = ok(&["dag", pipeline, saved, "--json"]);
    assert_eq!(fresh, read);
    assert!(read.contains("subject_A%5FB-ses_01-run_2.txt"), "{read}");
    assert!(read.contains("out/all/global.txt"), "{read}");
    let hints = ok(&["check", pipeline, "--json", "--hovers"]);
    assert!(
        hints.contains("out/session/subject_{sub}-ses_{ses}.txt"),
        "{hints}"
    );
    let hovers = ok(&["check", pipeline, "--json", "--hovers"]);
    // `{@entities}`'s word documentation names the `entities:` line that spells it.
    assert!(hovers.contains("\"@entities\":{"), "{hovers}");
    assert!(hovers.contains("`entities:` line spells it"), "{hovers}");
    assert!(hovers.contains("#entity-formatting"), "{hovers}");
    // A saved custom source rule retains its spelling when output formatting changes.
    tree.write("pipeline.spit", &format!(
        "entities: {{key}}-{{value}} separated \"_\" empty \"global\"\n{FLOW}default path: out/{{@product}}/{{@entities}}.txt\n"
    ));
    let changed = ok(&["dag", pipeline, saved, "--json"]);
    assert!(
        changed.contains("raw/subject_A%5FB-ses_01-run_2.txt"),
        "{changed}"
    );
    assert!(
        changed.contains("out/copied/sub-A%5FB_ses-01_run-2.txt"),
        "{changed}"
    );
}

#[test]
fn imports_keep_library_source_formats_and_use_caller_output_formats() {
    let tree = Tree::new("entities-imports", &[]);
    for (format, expected) in [(FORMAT, "raw/subject_{sub}.txt"), ("", "raw/sub-{sub}.txt")] {
        tree.write(
            "library.spit",
            &format!(
                "{format}source raw [sub]\npath raw: raw/{{@entities}}.txt\noperation copy(x)\n"
            ),
        );
        let main = tree.write("main.spit", "entities: {key}-{value} separated \"_\" empty \"global\"\nuse library.spit as lib\ncopied = lib::copy(lib::raw)\npath copied: out/{@product}/{@entities}\n");
        let pipeline = parse_pipeline_at(&std::fs::read_to_string(&main).unwrap(), &main).unwrap();
        assert_eq!(
            pipeline.path_template_for("lib::raw").unwrap().to_string(),
            expected
        );
        assert_eq!(
            pipeline.path_template_for("copied").unwrap().to_string(),
            "out/{@product}/sub-{sub}"
        );
    }
}

#[test]
fn aliases_can_precede_the_format_but_need_one_and_no_product_is_named_entities() {
    let pipeline = parse_pipeline("entities   sub: subject\nentities: {key}_{value} separated \"-\" empty \"global\"\nsource raw [sub]\noperation copy(x) -> Image\nmade : Image [sub] = copy(raw)\npath made: out/{@product}/{@entities}\n").unwrap();
    assert_eq!(
        pipeline.path_template_for("made").unwrap().to_string(),
        "out/{@product}/subject_{sub}"
    );
    let error = parse_pipeline(
        "entities sub: subject\nsource raw [sub]\noperation copy(x)\nentities = copy(raw)\n",
    )
    .unwrap_err();
    assert!(
        error.to_string().contains(
            "`entities` is one of SPIT's keywords, so no product may take it as its name"
        ),
        "{error}"
    );
    // A label renames a dimension in the format, so it needs one.
    let error = parse_pipeline(
        "entities sub: subject\nsource raw [sub]\noperation copy(x)\nmade = copy(raw)\n",
    )
    .unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(
        error
            .message()
            .contains("an entity label renames a dimension in the `entities:` format, and this pipeline declares none"),
        "{error}"
    );
}

#[test]
fn discovery_decodes_values_and_escaped_aliases_and_accepts_assignment_affixes() {
    let tree = Tree::new(
        "entities-escaping",
        &["raw/e_subject%5Fid:A%5Fb%2F%C3%A9!-e_ses:01!.txt"],
    );
    tree.write("pipeline.spit", "entities: e_{key}:{value}! separated \"-\" empty \"global\"\nentities sub: subject_id\nsource raw [sub, ses]\noperation copy(x)\ncopied = copy(raw)\npath copied: out/{@product}/{@entities}\n");
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\npath raw: raw/{@entities}.txt\n",
    );
    let inventory = ok(&["inputs", recipe.to_str().unwrap()]);
    assert!(inventory.contains("sub=A_b/é,ses=01"), "{inventory}");
    let dag = ok(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(
        dag.contains("e_subject%5Fid:A%5Fb%2F%C3%A9!-e_ses:01!"),
        "{dag}"
    );
}

#[test]
fn bad_declarations_are_located_and_recipe_or_stage_overrides_are_rejected() {
    for (declaration, message) in [
        ("entities: {key} separated \"_\" empty \"global\"", "then `{value}`"),
        ("entities: {value}_{key} separated \"-\" empty \"global\"", "then `{value}`"),
        (
            "entities: {key}_{value}_{value} separated \"-\" empty \"global\"",
            "once each",
        ),
        (
            "entities: {key}_{unknown} separated \"-\" empty \"global\"",
            "no other placeholders",
        ),
        (
            "entities: {key}_{value} separated by \"-\" empty \"global\"",
            "must be quoted",
        ),
        (
            "entities: {key}_{value} separated \"/\" empty \"global\"",
            "one path component",
        ),
        (
            "entities: {key}_{value} separated \"%\" empty \"global\"",
            "one path component",
        ),
        ("entities: {key}{value} separated \"-\" empty \"global\"", "ambiguous"),
        (
            "entities: {key}_{value} separated \"-\"",
            "after the entities separator, write `empty \"global\"`",
        ),
        ("entities: {key}_{value} separated \"-\" empty \"global\"\nentities other: subject", "unknown dimension"),
        ("entities: {key}_{value} separated \"-\" empty \"global\"\nentities sub: ses", "more than one dimension"),
        (
            "entities: {key}_{value} separated \"-\" empty \"global\"\nentities sub: subject\nentities sub: participant",
            "duplicate entity label",
        ),
        (
            "entities: {key}_{value} separated \"-\" empty \"global\"\nentities: {key}={value} separated \"__\" empty \"global\"",
            "one `entities:` format",
        ),
    ] {
        let error = parse_pipeline(&format!("source raw [sub, ses]\n{declaration}\n")).unwrap_err();
        assert!(error.line() >= 2, "{error}");
        assert!(error.message().contains(message), "{declaration}: {error}");
    }
    let error = parse_pipeline(
        "stage outputs:\n  entities: {key}_{value} separated \"-\" empty \"global\"\n",
    )
    .unwrap_err();
    assert_eq!(error.line(), 2);
    assert!(error.message().contains("whole pipeline"), "{error}");
    let error =
        spit::parse_input_spec("entities: {key}_{value} separated \"-\" empty \"global\"\n")
            .unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(error.message().contains("pipeline"), "{error}");

    let tree = Tree::new("entities-errors", &[]);
    let file = tree.write(
        "bad.spit",
        "source raw [sub]\nentities: {value}_{key} separated \"-\" empty \"global\"\n",
    );
    let output = spit(&["check", file.to_str().unwrap(), "--json", "--hovers"]);
    assert!(!output.status.success());
    let json = text(&output.stdout);
    assert!(json.contains("\"line\":2"), "{json}");
    assert!(json.contains("\"column\":"), "{json}");
    assert!(json.contains("\"word_docs\":"), "{json}");
}
