//! Custom entities agree across path hints, discovery and saved inventories.

mod support;

use spit::{inspect_paths, parse_pipeline, parse_pipeline_at, parse_source_inventory, resolve};
use support::{bound, spit, text, Tree};

const FORMAT: &str =
    "default entities: {@key}_{@value} separated \"-\"\ndefault entities sub: subject\n";
const FLOW: &str = "source raw [sub, ses, run]\nsource config\noperation copy(x)\noperation merge(x: many)\ncopied = copy(raw)\nsession = merge(copied @vary(run))\nsubject = merge(session @vary(ses))\nall = merge(subject @vary(sub))\n";

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
        "{FORMAT}{FLOW}default path: {{@product}}[/{{@entities}}].txt\npath config: config.txt\n"
    ))
    .unwrap();
    inspect_paths(&pipeline).unwrap();
    for (product, expected) in [
        ("copied", "{@product}/subject_{sub}-ses_{ses}-run_{run}.txt"),
        ("session", "{@product}/subject_{sub}-ses_{ses}.txt"),
        ("subject", "{@product}/subject_{sub}.txt"),
        ("all", "{@product}.txt"),
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
    assert!(paths.contains("all.txt"), "{paths}");
}

#[test]
fn a_dimensionless_product_drops_an_optional_group_and_output_rules_use_the_format() {
    let pipeline = parse_pipeline(&format!(
        "default entities: {{@key}}_{{@value}} separated \"-\"\n{FLOW}default path: out/[{{@entities}}_]{{@product}}.txt\npath raw: raw/{{@entities}}.txt\npath config: config.txt\n"
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

    // Outside a group, a product with no dimensions has no `{@entities}`.
    let pipeline = parse_pipeline("default entities: {@key}={@value} separated \"__\"\nsource config\npath config: in/{@entities}.txt\n").unwrap();
    assert_eq!(
        pipeline.path_template_for("config").unwrap().to_string(),
        "in/{@entities}.txt"
    );
    assert_eq!(
        inspect_paths(&pipeline).unwrap_err().to_string(),
        "path rule for `config` uses `{@entities}`, but `config` has no dimensions; put it in `[...]`, as `[{@entities}_]`"
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
        &format!("{FORMAT}{FLOW}default path: out/{{@product}}[/{{@entities}}].txt\n"),
    );
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\ninputs:\n    raw: raw/{@entities}.txt\n    config: config.txt\n",
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
    assert!(read.contains("out/all.txt"), "{read}");
    let hints = ok(&["check", pipeline, "--json", "--hovers"]);
    assert!(
        hints.contains("out/session/subject_{sub}-ses_{ses}.txt"),
        "{hints}"
    );
    let hovers = ok(&["check", pipeline, "--json", "--hovers"]);
    // `{@entities}`'s word documentation names the `default entities:` that spells it.
    assert!(hovers.contains("\"@entities\":{"), "{hovers}");
    assert!(
        hovers.contains("as the nearest `default entities:` spells it"),
        "{hovers}"
    );
    assert!(hovers.contains("#entity-formatting"), "{hovers}");
    // A saved custom source rule retains its spelling when output formatting changes.
    tree.write("pipeline.spit", &format!(
        "default entities: {{@key}}-{{@value}} separated \"_\"\n{FLOW}default path: out/{{@product}}[/{{@entities}}].txt\n"
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
        let main = tree.write("main.spit", "default entities: {@key}-{@value} separated \"_\"\nuse library.spit as lib\ncopied = lib::copy(lib::raw)\npath copied: out/{@product}/{@entities}\n");
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
    let pipeline = parse_pipeline("default entities sub: subject\ndefault entities: {@key}_{@value} separated \"-\"\nsource raw [sub]\noperation copy(x) -> Image\nmade : Image [sub] = copy(raw)\npath made: out/{@product}/{@entities}\n").unwrap();
    assert_eq!(
        pipeline.path_template_for("made").unwrap().to_string(),
        "out/{@product}/subject_{sub}"
    );
    let error = parse_pipeline(
        "default entities sub: subject\nsource raw [sub]\noperation copy(x)\nentities = copy(raw)\n",
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
        "default entities sub: subject\nsource raw [sub]\noperation copy(x)\nmade = copy(raw)\n",
    )
    .unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(
        error
            .message()
            .contains("an entity label renames a dimension in a `default entities:` spelling, and there is none"),
        "{error}"
    );
}

#[test]
fn discovery_decodes_values_and_escaped_aliases_and_accepts_assignment_affixes() {
    let tree = Tree::new(
        "entities-escaping",
        &["raw/e_subject%5Fid:A%5Fb%2F%C3%A9!-e_ses:01!.txt"],
    );
    tree.write("pipeline.spit", "default entities: e_{@key}:{@value}! separated \"-\"\ndefault entities sub: subject_id\nsource raw [sub, ses]\noperation copy(x)\ncopied = copy(raw)\npath copied: out/{@product}/{@entities}\n");
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\ninputs:\n    raw: raw/{@entities}.txt\n",
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
fn bad_declarations_are_located_and_old_forms_say_what_to_write() {
    for (declaration, message) in [
        ("default entities: {@key} separated \"_\"", "then `{@value}`"),
        ("default entities: {@value}_{@key} separated \"-\"", "then `{@value}`"),
        (
            "default entities: {@key}_{@value}_{@value} separated \"-\"",
            "once each",
        ),
        (
            "default entities: {@key}_{unknown} separated \"-\"",
            "no other placeholders",
        ),
        (
            "default entities: {key}_{value} separated \"-\"",
            "write `{@key}`: SPIT's own placeholders take `@`, and `{key}` would be a dimension",
        ),
        (
            "default entities: {@key}_{@value}_{value} separated \"-\"",
            "write `{@value}`: SPIT's own placeholders take `@`, and `{value}` would be a dimension",
        ),
        (
            "default entities: {@key}_{@value} separated by \"-\"",
            "must be quoted",
        ),
        (
            "default entities: {@key}_{@value}",
            "expected `default entities: {@key}={@value} separated \"__\"`",
        ),
        (
            "default entities: {@key}_{@value} separated \"/\"",
            "one path component",
        ),
        (
            "default entities: {@key}_{@value} separated \"%\"",
            "one path component",
        ),
        ("default entities: {@key}{@value} separated \"-\"", "ambiguous"),
        (
            "default entities: {@key}_{@value} separated \"-\" empty \"global\"",
            "unexpected text after the entities separator; a product with no dimensions has no `{@entities}`, so put it in `[...]`",
        ),
        (
            "entities: {@key}_{@value} separated \"-\"",
            "a default is written after `default`, as `default entities: ...`, or in a `default:` block",
        ),
        ("default entities: {@key}_{@value} separated \"-\"\ndefault entities other: subject", "unknown dimension"),
        ("default entities: {@key}_{@value} separated \"-\"\ndefault entities sub: ses", "more than one dimension"),
        (
            "default entities: {@key}_{@value} separated \"-\"\ndefault entities sub: subject\ndefault entities sub: participant",
            "duplicate entity label",
        ),
        (
            "default entities: {@key}_{@value} separated \"-\"\ndefault entities: {@key}={@value} separated \"__\"",
            "duplicate `default entities:`",
        ),
    ] {
        let error = parse_pipeline(&format!("source raw [sub, ses]\n{declaration}\n")).unwrap_err();
        assert!(error.line() >= 2, "{error}");
        assert!(error.message().contains(message), "{declaration}: {error}");
    }
    // A stage sets its spelling once, and with `default` like the file.
    let error = parse_pipeline(
        "source raw [sub]\nstage outputs:\n  default entities: {@key}_{@value} separated \"-\"\n  default entities: {@key}={@value} separated \"__\"\n",
    )
    .unwrap_err();
    assert_eq!(error.line(), 4);
    assert!(
        error
            .message()
            .contains("duplicate `default entities:` for stage `outputs`"),
        "{error}"
    );
    let error = parse_pipeline(
        "source raw [sub]\nstage outputs:\n  entities: {@key}_{@value} separated \"-\"\n",
    )
    .unwrap_err();
    assert_eq!(error.line(), 3);
    assert!(
        error
            .message()
            .contains("a default is written after `default`"),
        "{error}"
    );
    // So does a recipe.
    let error = spit::parse_input_spec("entities: {@key}_{@value} separated \"-\"\n").unwrap_err();
    assert_eq!(error.line(), 1);
    assert!(
        error
            .message()
            .contains("a default is written after `default`"),
        "{error}"
    );

    let tree = Tree::new("entities-errors", &[]);
    let file = tree.write(
        "bad.spit",
        "source raw [sub]\ndefault entities: {@value}_{@key} separated \"-\"\n",
    );
    let output = spit(&["check", file.to_str().unwrap(), "--json", "--hovers"]);
    assert!(!output.status.success());
    let json = text(&output.stdout);
    assert!(json.contains("\"line\":2"), "{json}");
    assert!(json.contains("\"column\":"), "{json}");
    assert!(json.contains("\"word_docs\":"), "{json}");
}

#[test]
fn a_stage_spelling_covers_its_products_and_the_stages_nested_in_it() {
    let pipeline = parse_pipeline(
        "default entities: {@key}_{@value} separated \"-\"\n\
         default path: out/{@product}/{@entities}.txt\n\
         source raw [sub, ses]\n\
         path raw: in/{@entities}.txt\n\
         operation copy(x)\n\
         first = copy(raw)\n\
         stage post:\n    \
             default entities: {@key}={@value} separated \"__\"\n    \
             default entities sub: subject\n    \
             second = copy(first)\n    \
             stage inner:\n        \
                 third = copy(second)\n",
    )
    .unwrap();
    inspect_paths(&pipeline).unwrap();
    for (product, expected) in [
        ("raw", "in/sub_{sub}-ses_{ses}.txt"),
        ("first", "out/{@product}/sub_{sub}-ses_{ses}.txt"),
        ("second", "out/{@product}/subject={sub}__ses={ses}.txt"),
        ("third", "out/{@product}/subject={sub}__ses={ses}.txt"),
    ] {
        assert_eq!(
            pipeline.path_template_for(product).unwrap().to_string(),
            expected,
            "{product}"
        );
    }
    let inventory = parse_source_inventory("sources:\n raw[sub=1,ses=2]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let paths = bound(&pipeline, &dag).unwrap();
    for path in [
        "out/first/sub_1-ses_2.txt",
        "out/second/subject=1__ses=2.txt",
        "out/third/subject=1__ses=2.txt",
    ] {
        assert!(paths.contains(path), "{path}: {paths}");
    }
}

#[test]
fn a_recipe_spelling_spells_the_recipes_own_source_rules() {
    let tree = Tree::new(
        "entities-recipe",
        &["rec/sub-1_ses-2.txt", "labelled/subject-1_ses-2.txt"],
    );
    tree.write(
        "pipeline.spit",
        "default entities: {@key}_{@value} separated \"-\"\ndefault path: out/{@product}/{@entities}.txt\nsource raw [sub, ses]\noperation copy(x)\nfirst = copy(raw)\n",
    );
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\ndefault entities: {@key}-{@value} separated \"_\"\ninputs:\n    raw: rec/{@entities}.txt\n",
    );
    let recipe = recipe.to_str().unwrap();
    let inventory = ok(&["inputs", recipe]);
    assert!(
        inventory.contains("raw: rec/sub-{sub}_ses-{ses}.txt"),
        "{inventory}"
    );
    assert!(inventory.contains("raw[sub=1,ses=2]"), "{inventory}");
    // The pipeline's spelling still spells the outputs.
    let paths = ok(&["dag", recipe, "--paths"]);
    assert!(paths.contains("path: rec/sub-1_ses-2.txt"), "{paths}");
    assert!(paths.contains("path: out/first/sub_1-ses_2.txt"), "{paths}");

    // A recipe's labels go with its spelling.
    let labelled = tree.write(
        "labelled.spitin",
        "pipeline pipeline.spit\nroot .\ndefault entities: {@key}-{@value} separated \"_\"\ndefault entities sub: subject\ninputs:\n    raw: labelled/{@entities}.txt\n",
    );
    let inventory = ok(&["inputs", labelled.to_str().unwrap()]);
    assert!(
        inventory.contains("raw: labelled/subject-{sub}_ses-{ses}.txt"),
        "{inventory}"
    );
    assert!(inventory.contains("raw[sub=1,ses=2]"), "{inventory}");

    // Without one, the recipe's rules take the pipeline's spelling.
    let plain = tree.write(
        "plain.spitin",
        "pipeline pipeline.spit\nroot .\ninputs:\n    raw: rec/{@entities}.txt\n",
    );
    let inventory = ok(&["inputs", plain.to_str().unwrap()]);
    assert!(
        inventory.contains("raw: rec/sub_{sub}-ses_{ses}.txt"),
        "{inventory}"
    );

    // A label in a recipe needs the recipe's own spelling.
    let error = spit::parse_input_spec(
        "pipeline pipeline.spit\ndefault entities sub: subject\ninputs:\n    raw: rec/{@entities}.txt\n",
    )
    .unwrap_err();
    assert_eq!(error.line(), 2);
    assert!(
        error
            .message()
            .contains("an entity label renames a dimension in a `default entities:` spelling, and there is none"),
        "{error}"
    );
}

#[test]
fn a_default_that_needs_entities_leaves_a_dimensionless_source_to_its_own_rule() {
    // As a default that needs `{@stage}` covers no source, one that needs
    // `{@entities}` covers no source without dimensions: a recipe gives it
    // its rule instead.
    let pipeline = parse_pipeline(
        "default entities: {@key}_{@value} separated \"-\"\n\
         default path: out/{@product}/{@entities}.txt\n\
         source raw [sub]\n\
         source config\n",
    )
    .unwrap();
    assert_eq!(
        pipeline.path_rule_for("raw").map(|rule| rule.as_str()),
        Some("out/{@product}/{@entities}.txt")
    );
    assert_eq!(pipeline.path_rule_for("config"), None);
    inspect_paths(&pipeline).unwrap();
}
