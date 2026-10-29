mod support;

use support::{errors, rendered};

use spit::{diagnose, DiagnosticSource};

#[test]
fn validates_external_inventory_and_semantics() {
    let text =
        "source raw : Image [id]\noperation copy(image: Image) -> Image\nresult = copy(raw)\ndefault entities: {@key}={@value} separated \"__\"\npath result: out/{@product}/{@entities}\n";
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
    let pipeline = "source raw [id]\noperation copy(input)\nresult = copy(raw)\ndefault entities: {@key}={@value} separated \"__\"\npath result: out/{@product}/{@entities}\n";
    let inventory = "sources:\n  raw[id=x]\n";
    let recipe = spit::parse_input_spec("inputs:\n    raw: data/{id}.txt\n").unwrap();
    let context = spit::Context {
        recipe: Some(&recipe),
        ..spit::Context::at(std::path::Path::new("pipeline.spit"))
    };
    let (checked, records) =
        spit::diagnose_checked_with_records(pipeline, inventory, context).unwrap();
    assert!(errors(checked.warnings).is_empty());
    // The recipe's rule for `raw` is not merged into the pipeline.
    assert_eq!(
        checked.pipeline.product_paths.keys().collect::<Vec<_>>(),
        ["result"]
    );
    assert_eq!(records.inventory.artifacts.len(), 1);
}

#[test]
fn type_errors_point_to_the_exact_flow_step_even_when_operation_is_reused() {
    let text = "source camera : Frame<Camera> [id]\n\
                source lidar : Frame<Lidar> [id]\n\
                operation inspect(frame: Frame<$Kind>) -> Checked<$Kind>\n\
                camera_checked = inspect(camera)\n\
                lidar_checked : Checked<Camera> [id] = inspect(lidar)\ndefault entities: {@key}={@value} separated \"__\"\npath camera_checked: out/{@product}/{@entities}\n";
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
                final = second(middle)\ndefault entities: {@key}={@value} separated \"__\"\npath middle: out/{@product}/{@entities}\n";
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
        Some("contexts:\n    [sub=01]:\n        [run=01]:\n            raw, raw\n"),
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

/// Two steps that make `y`, steps that read it directly and through
/// another, and a step on its own that has an error.
const TWO_PRODUCERS: &str = "\
source lines : Lines [site]
source table : Table [site]
operation clean(x: Lines) -> Lines
operation tab(x: Table) -> Table
operation count(x: Lines) -> Count
operation split(x: Lines) -> (a: Lines, b: Table)
";

#[test]
fn a_product_two_steps_make_quiets_every_step_that_reads_it() {
    let errors = |steps: &str| -> Vec<(usize, String)> {
        errors(diagnose(&format!("{TWO_PRODUCERS}{steps}"), None))
            .into_iter()
            .map(|error| (error.line.unwrap_or_default(), error.message))
            .collect()
    };
    let readers = "z = count(y)\nz2 = clean(y)\nzz = tab(z2)\nw = count(table)\n";
    // Whichever step comes first, neither's type reaches the readers of
    // `y`, and the step that reads only `table` is still checked.
    for producers in [
        "y = clean(lines)\ny = tab(table)\n",
        "y = tab(table)\ny = clean(lines)\n",
        "y, t = split(lines)\ny = tab(table)\nu = tab(t)\n",
        "y = tab(table)\ny, t = split(lines)\nu = tab(t)\n",
    ] {
        let found = errors(&format!("{producers}{readers}"));
        let w = 7 + producers.lines().count() + 3;
        assert_eq!(
            found,
            [
                (8, "duplicate product name `y`".to_owned()),
                (
                    w,
                    "type mismatch at `count.x`: product `table` is Table, expected Lines"
                        .to_owned()
                ),
            ],
            "{producers}"
        );
        // Nor can it be resolved.
        let pipeline =
            spit::parse_pipeline(&format!("{TWO_PRODUCERS}{producers}{readers}")).unwrap();
        assert!(spit::validate_pipeline(&pipeline).is_err(), "{producers}");
    }
}

#[test]
fn missing_join_input_points_to_the_call() {
    let text = "source raw [id]\nsource reference [id]\noperation join(left, right)\nresult = join(raw, reference)\ndefault entities: {@key}={@value} separated \"__\"\npath result: out/{@product}/{@entities}\n";
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
        "require [site] where raw count>=1\nrequire [site] where raw count>=2\n",
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
default path: {@product}/{@entities}.csv
path raw: in/{id}.csv
operation clean(table: Table) -> Table
command clean: tool {table} {result}
operation join(table: Table, table2: Table) -> Table
cleaned = clean(raw)
joined = join(cleaned)
typo = clean(rwa)
default entities: {@key}={@value} separated \"__\"\n";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "error: line 3: path template for `raw` omits dimension `batch`; artifacts differing only in `batch` would share a path",
            "error: line 5: command for `clean` uses unknown placeholder `{result}`: `clean` has no port `result`, and the jobs of `cleaned` have `id`, `batch`",
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
command copy: tool --color=#fff {input} {@output}# note
result = copy(raw)
source spare [id]# note
default entities: {@key}={@value} separated \"__\"
path result: out/{@product}/{@entities}
";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "warning: line 3: `#` after `{@output}` is part of that word, not a comment; put a space before `#` to start a comment, or quote the text to keep it",
            "error: line 5: expected closing `]` in product declaration (`#` after `[id]` is part of that word, not a comment; put a space before `#` to start a comment, or quote the text to keep it)",
        ]
    );
    let quoted = text
        .replace("{@output}# note", "{@output} # note")
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
default path: {@entities}.csv
operation clean(input)
cleaned = clean(raw)
source other [id]
path other: {@product}/{id}/{shard}.csv
default entities: {@key}={@value} separated \"__\"\n";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "error: line 2: products `raw` and `cleaned` bind to the same path `id=id.csv` for the same entities; include `{@product}` or distinguish their path rules",
            "warning: line 5: source product `other` is never used as an input",
            "error: line 6: path template for `other` uses absent dimension `shard`; put it in `[...]` if only some products have it",
        ]
    );
}

#[test]
fn a_cycle_is_reported_once_at_the_step_it_was_found_at() {
    let text = "operation copy(input: A) -> A\n\
                a = copy(b)\n\
                b = copy(c)\n\
                c = copy(a)\n\
                path b: out/b.txt\n\
                path c: out/c.txt\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(2));
    assert_eq!(issues[0].columns, Some(0..1));
    assert_eq!(issues[0].message, "pipeline cycle: a -> b -> c -> a");
}

#[test]
fn every_independent_cycle_is_reported_once_and_quiets_what_reads_it() {
    let text = "\
default path: out/{@product}/{site}
source scan : Scan [site]
operation copy(x: Scan) -> Scan
operation two(x: Scan, y: Scan) -> Scan
operation take(x: Photo) -> Photo
c = copy(c)
d = two(e, d)
e = take(d)
f = take(e)
g = take(scan)
";
    let found: Vec<_> = errors(diagnose(text, None))
        .into_iter()
        .map(|error| (error.line.unwrap_or_default(), error.message))
        .collect();
    // `d` reads itself and `e`, two loops in one group of steps, which is
    // said once. `e` is never checked against `d` in source order, `f` reads
    // from the loop, and `g` reads from neither, so it is still checked.
    assert_eq!(
        found,
        [
            (6, "pipeline cycle: c -> c".to_owned()),
            (7, "pipeline cycle: d -> e -> d".to_owned()),
            (
                10,
                "type mismatch at `take.x`: product `scan` is Scan, expected Photo".to_owned()
            ),
        ]
    );
    // Nothing resolves a pipeline with a cycle, so no DAG is made from it.
    let pipeline = spit::parse_pipeline(text).unwrap();
    assert!(matches!(
        spit::validate_pipeline(&pipeline),
        Err(spit::ResolveError::Cycle { .. })
    ));
    assert!(matches!(
        spit::resolve(&pipeline, &spit::SourceInventory::default()),
        Err(spit::ResolveError::Cycle { .. })
    ));
}

#[test]
fn independent_errors_on_one_line_are_each_reported() {
    // The default rule has a shape, which only a source's rule may, and apart
    // from that it gives the source and the output one path.
    let text = "source s [d]\ndefault path: in/{d:digits}.txt\noperation f(s) -> .txt\ncommand f: f {s} {@output}\no = f(s)\n";
    let found = errors(diagnose(text, None));
    let said: Vec<_> = found
        .iter()
        .map(|error| (error.line, error.columns.clone()))
        .collect();
    assert_eq!(
        said,
        [(Some(2), Some(17..27)), (Some(2), Some(14..31))],
        "{found:?}"
    );
    assert!(found[0].message.contains("`{d:digits}` has a shape"));
    assert!(found[1]
        .message
        .contains("products `s` and `o` bind to the same path `in/d.txt`"));
}

#[test]
fn a_misspelled_dimension_is_not_also_reported_as_omitted() {
    let text = "default path: out/[ses-{sess}]{@product}.txt\nsource raw [ses]\n";
    assert_eq!(
        rendered(&diagnose(text, None)),
        ["error: line 1: path rule names `{sess}`, which no product has"]
    );
}

#[test]
fn unrelated_errors_at_one_span_are_each_reported() {
    // `wrapped` has two faults of its own: its type is left open, and it has
    // no path rule. Both point at the same text.
    let text = "types strict\nsource raw : Image [sub]\noperation wrap(x: Image) -> Frame<T>\ncommand wrap: w {x} {@output}\nwrapped = wrap(raw)\ntype Frame<Kind>\n";
    let found = errors(diagnose(text, None));
    let said: Vec<_> = found.iter().map(|error| error.line).collect();
    assert_eq!(said, [Some(5), Some(5)], "{found:?}");
    assert!(found[0]
        .message
        .contains("`types strict` needs a known type for `wrapped`"));
    assert!(found[1]
        .message
        .contains("output `wrapped` has no path rule"));
}
