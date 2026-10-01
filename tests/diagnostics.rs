mod support;

use support::{errors, rendered};

use spit::{diagnose, DiagnosticSource};

#[test]
fn validates_external_inventory_and_semantics() {
    let text =
        "source raw : Image [id]\noperation copy(image: Image) -> Image\nresult = copy(raw)\n";
    let bad_inventory = "sources:\n  raw[id=x,id=y]\n";
    let issues = errors(diagnose(text, Some(bad_inventory)));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Inventory);
    assert_eq!(issues[0].line, Some(2));

    let good_inventory = "sources:\n  raw[id=x]\n";
    assert!(errors(diagnose(text, Some(good_inventory))).is_empty());

    let bad_pipeline = text.replace("raw : Image", "raw : Other");
    let issues = errors(diagnose(&bad_pipeline, Some(good_inventory)));
    assert_eq!(issues[0].line, Some(3));
    assert!(issues[0].message.contains("type mismatch"));
}

#[test]
fn checked_diagnosis_retains_the_original_pipeline_and_inventory() {
    let pipeline = "source raw [id]\noperation copy(input)\nresult = copy(raw)\n";
    let inventory = "sources:\n  raw[id=x]\n";
    let recipe = spit::parse_input_spec("path raw: data/{id}.txt\n").unwrap();
    let context = spit::Context {
        recipe: Some(&recipe),
        ..spit::Context::at(std::path::Path::new("pipeline.spit"))
    };
    let (checked, records) =
        spit::diagnose_checked_with_records(pipeline, inventory, context).unwrap();
    assert!(errors(checked.warnings).is_empty());
    assert!(checked.pipeline.product_paths.is_empty());
    assert_eq!(records.inventory.artifacts.len(), 1);
}

#[test]
fn type_errors_point_to_the_exact_flow_step_even_when_operation_is_reused() {
    let text = "source camera : Frame<Camera> [id]\n\
                source lidar : Frame<Lidar> [id]\n\
                operation inspect(frame: Frame<$Kind>) -> Checked<$Kind>\n\
                camera_checked = inspect(camera)\n\
                lidar_checked : Checked<Camera> [id] = inspect(lidar)\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(5));
    assert!(issues[0].message.contains("type conflict"));
}

#[test]
fn inferred_type_error_points_to_the_consuming_step() {
    let text = "source raw : A<Native> [id]\n\
                operation first(a: A<X>) -> B<X>\n\
                operation second(b: B<Standard>) -> C\n\
                middle = first(raw)\n\
                final = second(middle)\n";
    let issues = errors(diagnose(text, Some("sources:\n")));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(5));
    assert!(issues[0].message.contains("B<Native>"));
}

#[test]
fn source_inventory_errors_point_to_the_source_line() {
    let pipeline = "source raw [id]\n";
    let unknown = errors(diagnose(pipeline, Some("sources:\n  other[id=x]\n")));
    assert_eq!(unknown[0].source, DiagnosticSource::Inventory);
    assert_eq!(unknown[0].line, Some(2));

    let duplicate = errors(diagnose(
        pipeline,
        Some("sources:\n  raw[id=x]\n  raw[id=x]\n"),
    ));
    assert_eq!(duplicate[0].source, DiagnosticSource::Inventory);
    assert_eq!(duplicate[0].line, Some(3));

    let nested = errors(diagnose(
        "source raw [sub, run]\n",
        Some("contexts sessions:\n    [sub=01]:\n        [run=01]:\n            raw, raw\n"),
    ));
    assert_eq!(nested[0].source, DiagnosticSource::Inventory);
    assert_eq!(nested[0].line, Some(4));
}

#[test]
fn duplicate_declaration_points_to_the_second_declaration() {
    let issues = errors(diagnose("source raw [id]\nsource raw [id]\n", None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(2));
    assert!(issues[0].message.contains("duplicate product name"));
}

#[test]
fn missing_join_input_points_to_the_call() {
    let text = "source raw [id]\nsource reference [id]\noperation join(left, right)\nresult = join(raw, reference)\n";
    let issues = errors(diagnose(text, Some("sources:\n  raw[id=x]\n")));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(4));
    assert!(issues[0]
        .message
        .contains("no `reference` artifact for input `right` of `join`"));
}

#[test]
fn a_coverage_error_names_the_failing_rule_when_rules_share_a_product() {
    let recipe = spit::parse_input_spec(
        "require raw count>=1 per [site]\nrequire raw count>=2 per [site]\n",
    )
    .unwrap();
    let context = spit::Context {
        recipe: Some(&recipe),
        ..spit::Context::at(std::path::Path::new("a.spit"))
    };
    let issues = errors(spit::diagnose_in(
        "source raw [site, run]\n",
        Some("sources:\n  raw[site=A,run=1]\n"),
        context,
    ));
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("expected at least 2"));
}

#[test]
fn every_semantic_error_is_reported_once_in_line_order() {
    let text = "\
source raw : Table [id, batch]
path: {product}/{entities}.csv
path raw: in/{id}.csv
operation clean(table: Table) -> Table
command clean: tool {table} {result}
operation join(table: Table, table2: Table) -> Table
cleaned = clean(raw)
joined = join(cleaned)
typo = clean(rwa)
";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "error: line 3: path template for `raw` omits dimension `batch`; artifacts differing only in `batch` would share a path",
            "error: line 5: command for `clean` uses unknown placeholder `{result}`",
            "warning: line 6: operation `join` has no command, so its jobs cannot run",
            "error: line 8: unsupported shape for `join`: expected 2 input bindings, found 1",
            "error: line 9: unknown product `rwa`",
        ]
    );
}

#[test]
fn uses_of_a_failed_declaration_or_step_are_not_reported_again() {
    let text = "\
source raw : Table [id]
operation pair(a: Table, a: Table) -> Table
first = pair(raw, raw)
second = pair(first, first)
operation copy(table: Table) -> Table
third = copy(missing)
fourth = copy(third)
";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "error: line 2: operation `pair` has more than one port named `a`",
            "error: line 6: unknown product `missing`",
        ]
    );
}

#[test]
fn undeclared_operation_is_reported_when_its_name_prefixes_an_invalid_one() {
    let text = "source raw [id]\noperation copy_all(input) -> Image extra\nresult = copy(raw)\n";
    let issues = diagnose(text, None);
    assert_eq!(issues.len(), 2, "{issues:?}");
    assert_eq!(issues[1].line, Some(3));
    assert!(issues[1]
        .message
        .contains("operation `copy` must be declared"));
}

#[test]
fn source_with_wrong_dimensions_points_to_its_inventory_line() {
    let pipeline = "source raw [id]\n";
    let issues = diagnose(pipeline, Some("sources:\n  raw[id=x]\n  raw[other=y]\n"));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].source, DiagnosticSource::Inventory);
    assert_eq!(issues[0].line, Some(3));
    assert!(issues[0]
        .message
        .contains("must bind exactly the dimensions"));
}

#[test]
fn hash_ending_a_word_is_flagged_as_a_likely_comment() {
    let text = "\
source raw [id]
operation copy(input)
command copy: tool --color=#fff {input} {output}# note
result = copy(raw)
source spare [id]# note
";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "warning: line 3: `#` after `{output}` is part of that word, not a comment; put a space before `#` to start a comment, or quote the text to keep it",
            "error: line 5: expected closing `]` in product declaration (`#` after `[id]` is part of that word, not a comment; put a space before `#` to start a comment, or quote the text to keep it)",
        ]
    );
    let quoted = text
        .replace("{output}# note", "{output} # note")
        .replace("[id]# note", "[id] # note");
    assert_eq!(
        rendered(&diagnose(&quoted, None)),
        ["warning: line 5: source product `spare` is never used as an input"]
    );
}

#[test]
fn path_rule_errors_point_to_the_rule_in_use() {
    let text = "\
source raw [id]
path: {entities}.csv
operation clean(input)
cleaned = clean(raw)
source other [id]
path other: {product}/{id}/{shard}.csv
";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "error: line 2: products `raw` and `cleaned` bind to the same path `id=id.csv` for the same entities; include `{product}` or distinguish their path rules",
            "warning: line 5: source product `other` is never used as an input",
            "error: line 6: path template for `other` uses absent dimension `shard`",
        ]
    );
}

#[test]
fn a_cycle_is_reported_once_at_the_step_it_was_found_at() {
    let text = "operation copy(input: A) -> A\n\
                a = copy(b)\n\
                b = copy(c)\n\
                c = copy(a)\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(2));
    assert_eq!(issues[0].columns, Some(0..1));
    assert_eq!(issues[0].message, "pipeline cycle: a -> b -> c -> a");
}
