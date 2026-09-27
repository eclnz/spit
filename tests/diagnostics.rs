use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::{Command, Stdio};

use spit::{diagnose, Diagnostic, DiagnosticSource};

/// The errors among `diagnostics`. These tests pin where errors land;
/// warnings have their own tests.
fn errors(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.into_iter().filter(Diagnostic::is_error).collect()
}

#[test]
fn reports_syntax_line_from_unsaved_text() {
    let text = "source raw [id]\noperation copy(one)\nresult = copy(raw @ vary(id, extra))\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(3));
}

#[test]
fn reports_independent_syntax_errors_across_a_pipeline() {
    let text =
        "source raw [id]\nthis is invalid\noperation copy(one)\nalso invalid\nresult = copy(raw)\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(
        issues.iter().map(|issue| issue.line).collect::<Vec<_>>(),
        [Some(2), Some(4)]
    );
    assert!(issues
        .iter()
        .all(|issue| issue.source == DiagnosticSource::Pipeline));
}

#[test]
fn reports_pipeline_and_inventory_syntax_errors_together() {
    let pipeline = "source raw [id]\nthis is invalid\n";
    let inventory = "sources:\n  raw[id=x,id=y]\n  raw[id=a,id=b]\n";
    let issues = errors(diagnose(pipeline, Some(inventory)));
    assert_eq!(issues.len(), 3);
    assert_eq!(
        (issues[0].source, issues[0].line),
        (DiagnosticSource::Pipeline, Some(2))
    );
    assert_eq!(
        (issues[1].source, issues[1].line),
        (DiagnosticSource::Inventory, Some(2))
    );
    assert_eq!(
        (issues[2].source, issues[2].line),
        (DiagnosticSource::Inventory, Some(3))
    );
}

#[test]
fn reports_multiple_errors_in_sectioned_and_embedded_inventory_text() {
    let text = "products:\n  raw [id]\n  bad product\noperations:\n  copy(one)\n  bad operation\npipeline:\n  result = copy(raw)\nsources:\n  raw[id=x,id=y]\n  raw[id=a,id=b]\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(
        issues.iter().map(|issue| issue.line).collect::<Vec<_>>(),
        [Some(3), Some(6), Some(10), Some(11)]
    );
}

#[test]
fn seeded_deletions_report_every_damaged_pipeline_line() {
    let mut original = vec![
        "source raw [id]".to_owned(),
        "operation copy(one)".to_owned(),
    ];
    original.extend((0..24).map(|index| format!("step_{index:02} = copy(raw)")));
    assert!(errors(diagnose(&original.join("\n"), None)).is_empty());

    let mut seed = 0x5eed_2026_u64;
    for damaged_count in 1..=24 {
        let mut lines = original.clone();
        let mut damaged = BTreeSet::new();
        let mut expected_messages = BTreeMap::new();
        while damaged.len() < damaged_count {
            damaged.insert(2 + (next_random(&mut seed) as usize % 24));
        }
        for &index in &damaged {
            let (marker, expected) = [
                (" = ", "expected `=`"),
                ("(", "expected `(`"),
                (")", "expected closing `)`"),
                ("copy", "invalid operation name"),
            ][(next_random(&mut seed) % 4) as usize];
            lines[index] = lines[index].replacen(marker, "", 1);
            expected_messages.insert(index + 1, expected);
        }
        let issues = errors(diagnose(&lines.join("\n"), None));
        let actual: BTreeSet<_> = issues.iter().map(|issue| issue.line.unwrap()).collect();
        let expected: BTreeSet<_> = damaged.iter().map(|index| index + 1).collect();
        assert_eq!(actual, expected, "{damaged_count} damaged pipeline lines");
        for issue in &issues {
            assert!(
                issue
                    .message
                    .contains(expected_messages[&issue.line.unwrap()]),
                "line {:?}: {}",
                issue.line,
                issue.message
            );
        }
    }
}

#[test]
fn seeded_deletions_report_every_damaged_inventory_line() {
    let pipeline = "source raw [id]\n";
    let mut original = vec!["sources:".to_owned()];
    original.extend((0..24).map(|index| format!("raw[id={index:02}]")));
    assert!(errors(diagnose(pipeline, Some(&original.join("\n")))).is_empty());

    let mut seed = 0x1a2b_3c4d_u64;
    for damaged_count in 1..=24 {
        let mut lines = original.clone();
        let mut damaged = BTreeSet::new();
        let mut expected_messages = BTreeMap::new();
        while damaged.len() < damaged_count {
            damaged.insert(1 + (next_random(&mut seed) as usize % 24));
        }
        for &index in &damaged {
            let (marker, expected) = [
                ("[", "expected source artifact"),
                ("]", "expected closing `]`"),
                ("=", "expected `dimension=value`"),
                ("raw", "invalid source product"),
            ][(next_random(&mut seed) % 4) as usize];
            lines[index] = lines[index].replacen(marker, "", 1);
            expected_messages.insert(index + 1, expected);
        }
        let issues = errors(diagnose(pipeline, Some(&lines.join("\n"))));
        let actual: BTreeSet<_> = issues.iter().map(|issue| issue.line.unwrap()).collect();
        let expected: BTreeSet<_> = damaged.iter().map(|index| index + 1).collect();
        assert_eq!(actual, expected, "{damaged_count} damaged inventory lines");
        assert!(issues
            .iter()
            .all(|issue| issue.source == DiagnosticSource::Inventory));
        for issue in &issues {
            assert!(
                issue
                    .message
                    .contains(expected_messages[&issue.line.unwrap()]),
                "line {:?}: {}",
                issue.line,
                issue.message
            );
        }
    }
}

#[test]
fn deletion_messages_name_the_missing_syntax_without_cascading() {
    let original: Vec<String> = [
        "source raw : Image [id]",
        "require raw count>=1 per [id]",
        "path: out/{product}/{id}.txt",
        "operation copy(input: Image) -> Image",
        "command copy: tool {input} {output}",
        "result = copy(raw)",
        "sources:",
        "  raw[id=A]",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert!(errors(diagnose(&original.join("\n"), None)).is_empty());
    let cases = [
        (0, "]", "closing `]` in product declaration"),
        (0, "Image", "expected type name"),
        (1, "]", "closing `]` in constraint dimensions"),
        (2, ":", "expected `:` after path product"),
        (3, "(", "expected `(` after operation name"),
        (3, "->", "expected `->` before operation output type"),
        (4, ":", "expected command"),
        (5, "=", "expected `=` before operation call"),
        (5, ")", "expected closing `)`"),
        (7, "=", "expected `dimension=value`"),
        (7, "]", "expected closing `]` in source artifact"),
    ];
    for (line, deleted, expected) in cases {
        let mut lines = original.clone();
        lines[line] = lines[line].replacen(deleted, "", 1);
        let issues = errors(diagnose(&lines.join("\n"), None));
        assert_eq!(
            issues.len(),
            1,
            "deleting {deleted:?} on line {}: {issues:?}",
            line + 1
        );
        assert_eq!(issues[0].line, Some(line + 1));
        assert!(
            issues[0].message.contains(expected),
            "deleting {deleted:?} on line {}: {}",
            line + 1,
            issues[0].message
        );
    }
}

#[test]
fn sectioned_step_without_equals_names_the_missing_character() {
    let text = "products:\n  raw [id]\n  result [id]\noperations:\n  copy(one)\npipeline:\n  result copy(raw)\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(7));
    assert!(issues[0].message.contains("expected `=`"));
}

fn next_random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn validates_external_inventory_and_semantics() {
    let text = "source raw : Image [id]\noperation copy(Image) -> Image\nresult = copy(raw)\n";
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
fn type_errors_point_to_the_exact_flow_step_even_when_operation_is_reused() {
    let text = "source camera : Frame<Camera> [id]\n\
                source lidar : Frame<Lidar> [id]\n\
                operation inspect(Frame<$Kind>) -> Checked<$Kind>\n\
                camera_checked = inspect(camera)\n\
                lidar_checked : Checked<Camera> [id] = inspect(lidar)\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(5));
    assert!(issues[0].message.contains("type conflict"));
}

#[test]
fn inferred_type_error_points_to_the_consuming_sectioned_step() {
    let text = "products:\n\
                  raw : A<Native> [id]\n\
                  middle : Unknown [id]\n\
                  final : Unknown [id]\n\
                operations:\n\
                  first(A<X>) -> B<X>\n\
                  second(B<Standard>) -> C\n\
                pipeline:\n\
                  middle = first(raw)\n\
                  final = second(middle)\n";
    let issues = errors(diagnose(text, Some("sources:\n")));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(10));
    assert!(issues[0].message.contains("B<Native>"));
}

#[test]
fn source_inventory_errors_point_to_the_source_line() {
    let pipeline = "source raw [id]\n";
    let unknown = errors(diagnose(pipeline, Some("sources:\n  other[id=x]\n")));
    assert_eq!(unknown[0].source, DiagnosticSource::Inventory);
    assert_eq!(unknown[0].line, Some(2));

    let duplicate = errors(diagnose(pipeline, Some("sources:\n  raw[id=x]\n  raw[id=x]\n")));
    assert_eq!(duplicate[0].source, DiagnosticSource::Inventory);
    assert_eq!(duplicate[0].line, Some(3));
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
    let text = "source raw [id]\nsource reference [id]\noperation join(left: one, right: one)\nresult = join(raw, reference)\n";
    let issues = errors(diagnose(text, Some("sources:\n  raw[id=x]\n")));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(4));
    assert!(issues[0].message.contains("no `reference` artifact for input `right` of `join`"));
}

#[test]
fn coverage_error_points_to_the_failing_rule_when_rules_share_a_product() {
    let text = "source raw [site, run]\nrequire raw count>=1 per [site]\nrequire raw count>=2 per [site]\n";
    let issues = errors(diagnose(text, Some("sources:\n  raw[site=A,run=1]\n")));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(3));
    assert!(issues[0].message.contains("expected at least 2"));
}

#[test]
fn cli_accepts_stdin_and_returns_json() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["diagnose", "unsaved.spit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"source raw [id]\nthis is invalid\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"source\":\"pipeline\""));
    assert!(json.contains("\"line\":2"));
    assert!(json.contains("\"message\":\"expected source"));
}

#[test]
fn cli_reports_semantic_error_line_in_json() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["diagnose", "unsaved.spit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            b"source raw : A<Native> [id]\noperation first(A<X>) -> B<X>\nmiddle = first(raw)\noperation second(B<Standard>) -> C\nfinal = second(middle)\n",
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"line\":5"), "{json}");
    assert!(json.contains("B<Native>"), "{json}");
}

fn rendered(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics.iter().map(ToString::to_string).collect()
}

#[test]
fn every_semantic_error_is_reported_once_in_line_order() {
    let text = "\
source raw : Table [id, batch]
path: {product}/{entities}.csv
path raw: in/{id}.csv
operation clean(Table) -> Table
command clean: tool {input} {result}
operation join(Table, Table) -> Table
cleaned = clean(raw)
joined = join(cleaned)
typo = clean(rwa)
require raw count=1 per [shard]
";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "error: line 3: path template for `raw` omits dimension `batch`; artifacts differing only in `batch` would share a path",
            "error: line 5: command for `clean` uses unknown placeholder `{result}`",
            "warning: line 6: operation `join` has no command, so `bash` cannot run its jobs",
            "error: line 8: unsupported shape for `join`: expected 2 input bindings, found 1",
            "error: line 9: unknown product `rwa`",
            "error: line 10: coverage rule for `raw` must group by distinct dimensions of that product",
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
operation copy(Table) -> Table
third = copy(missing)
fourth = copy(third)
";
    assert_eq!(
        rendered(&diagnose(text, None)),
        [
            "error: line 2: operation `pair` has invalid input ports",
            "error: line 6: unknown product `missing`",
        ]
    );
}

#[test]
fn warnings_flag_unused_definitions_missing_commands_and_unbound_type_variables() {
    let text = "\
source raw : Table [id]
source spare : Table [id]
operation clean(Table) -> Table
operation tag(Table) -> Tagged<$Key>
operation unused(Table) -> Table
command clean: tool {input} {output}
cleaned = clean(raw)
tagged : Tagged<Label> [id] = tag(cleaned)
";
    let diagnostics = diagnose(text, None);
    assert_eq!(
        rendered(&diagnostics),
        [
            "warning: line 2: source product `spare` is never used as an input",
            "warning: line 4: operation `tag` has no command, so `bash` cannot run its jobs",
            "warning: line 4: output type variable `Key` of `tag` appears in no input; it is known only where the output product declares its type",
            "warning: line 5: operation `unused` is declared but never used",
        ]
    );
    assert!(errors(diagnostics).is_empty());

    // A pipeline without commands may be meant for its DAG alone.
    let without_commands = text.replace("command clean: tool {input} {output}\n", "");
    assert!(!rendered(&diagnose(&without_commands, None))
        .iter()
        .any(|line| line.contains("has no command")));

    // A file with no steps is a library of definitions, used by importing it.
    let library = "source raw : Table [id]\noperation clean(Table) -> Table\n";
    assert!(diagnose(library, None).is_empty());
}

#[test]
fn a_line_with_an_error_shows_no_warnings() {
    let text = "source raw : Table [id]\nsource spare : Table [id, id]\noperation clean(Table) -> Table\ncleaned = clean(raw)\n";
    assert_eq!(
        rendered(&diagnose(text, None)),
        ["error: line 2: product `spare` has duplicate or empty dimensions"]
    );
}

#[test]
fn cli_json_includes_each_severity_and_its_columns() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["diagnose", "unsaved.spit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"source raw [id]\nsource spare [id]\noperation copy(one)\nresult = copy(rwa)\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"diagnostics\":[\
{\"severity\":\"warning\",\"source\":\"pipeline\",\"line\":1,\"column\":8,\"end_column\":11,\"message\":\"source product `raw` is never used as an input\"},\
{\"severity\":\"warning\",\"source\":\"pipeline\",\"line\":2,\"column\":8,\"end_column\":13,\"message\":\"source product `spare` is never used as an input\"},\
{\"severity\":\"error\",\"source\":\"pipeline\",\"line\":4,\"column\":15,\"end_column\":18,\"message\":\"unknown product `rwa`\"}]}\n"
    );
}

#[test]
fn cli_json_columns_count_utf16_code_units() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["diagnose", "unsaved.spit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("source raw [id]\noperation copy(one)\nx = copy(résumé)\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let json = String::from_utf8(output.stdout).unwrap();
    // `é` is two bytes but one UTF-16 code unit, so `résumé` spans 10..16.
    assert!(json.contains("\"line\":3,\"column\":10,\"end_column\":16"), "{json}");
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
operation copy(one)
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
operation clean(one)
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
