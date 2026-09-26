use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::{Command, Stdio};

use spit::diagnose;

#[test]
fn reports_syntax_line_from_unsaved_text() {
    let text = "source raw [id]\noperation copy(one)\nresult = copy(raw @ vary(id, extra))\n";
    let issues = diagnose(text, None);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, "pipeline");
    assert_eq!(issues[0].line, Some(3));
}

#[test]
fn reports_independent_syntax_errors_across_a_pipeline() {
    let text =
        "source raw [id]\nthis is invalid\noperation copy(one)\nalso invalid\nresult = copy(raw)\n";
    let issues = diagnose(text, None);
    assert_eq!(
        issues.iter().map(|issue| issue.line).collect::<Vec<_>>(),
        [Some(2), Some(4)]
    );
    assert!(issues.iter().all(|issue| issue.source == "pipeline"));
}

#[test]
fn reports_pipeline_and_inventory_syntax_errors_together() {
    let pipeline = "source raw [id]\nthis is invalid\n";
    let inventory = "sources:\n  raw[id=x,id=y]\n  raw[id=a,id=b]\n";
    let issues = diagnose(pipeline, Some(inventory));
    assert_eq!(issues.len(), 3);
    assert_eq!((issues[0].source, issues[0].line), ("pipeline", Some(2)));
    assert_eq!((issues[1].source, issues[1].line), ("inventory", Some(2)));
    assert_eq!((issues[2].source, issues[2].line), ("inventory", Some(3)));
}

#[test]
fn reports_multiple_errors_in_sectioned_and_embedded_inventory_text() {
    let text = "products:\n  raw [id]\n  bad product\noperations:\n  copy(one)\n  bad operation\npipeline:\n  result = copy(raw)\nsources:\n  raw[id=x,id=y]\n  raw[id=a,id=b]\n";
    let issues = diagnose(text, None);
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
    assert!(diagnose(&original.join("\n"), None).is_empty());

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
        let issues = diagnose(&lines.join("\n"), None);
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
    assert!(diagnose(pipeline, Some(&original.join("\n"))).is_empty());

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
        let issues = diagnose(pipeline, Some(&lines.join("\n")));
        let actual: BTreeSet<_> = issues.iter().map(|issue| issue.line.unwrap()).collect();
        let expected: BTreeSet<_> = damaged.iter().map(|index| index + 1).collect();
        assert_eq!(actual, expected, "{damaged_count} damaged inventory lines");
        assert!(issues.iter().all(|issue| issue.source == "inventory"));
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
        "path: out/{id}.txt",
        "operation copy(input: Image) -> Image",
        "command copy: tool {input} {output}",
        "result = copy(raw)",
        "sources:",
        "  raw[id=A]",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert!(diagnose(&original.join("\n"), None).is_empty());
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
        let issues = diagnose(&lines.join("\n"), None);
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
    let issues = diagnose(text, None);
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
    let issues = diagnose(text, Some(bad_inventory));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, "inventory");
    assert_eq!(issues[0].line, Some(2));

    let good_inventory = "sources:\n  raw[id=x]\n";
    assert!(diagnose(text, Some(good_inventory)).is_empty());

    let bad_pipeline = text.replace("raw : Image", "raw : Other");
    let issues = diagnose(&bad_pipeline, Some(good_inventory));
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
    let issues = diagnose(text, None);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, "pipeline");
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
                  second(B<MNI>) -> C\n\
                pipeline:\n\
                  middle = first(raw)\n\
                  final = second(middle)\n";
    let issues = diagnose(text, Some("sources:\n"));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, "pipeline");
    assert_eq!(issues[0].line, Some(10));
    assert!(issues[0].message.contains("B<Native>"));
}

#[test]
fn source_inventory_errors_point_to_the_source_line() {
    let pipeline = "source raw [id]\n";
    let unknown = diagnose(pipeline, Some("sources:\n  other[id=x]\n"));
    assert_eq!(unknown[0].source, "inventory");
    assert_eq!(unknown[0].line, Some(2));

    let duplicate = diagnose(pipeline, Some("sources:\n  raw[id=x]\n  raw[id=x]\n"));
    assert_eq!(duplicate[0].source, "inventory");
    assert_eq!(duplicate[0].line, Some(3));
}

#[test]
fn duplicate_declaration_points_to_the_second_declaration() {
    let issues = diagnose("source raw [id]\nsource raw [id]\n", None);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(2));
    assert!(issues[0].message.contains("duplicate product name"));
}

#[test]
fn missing_join_input_points_to_the_call() {
    let text = "source raw [id]\nsource reference [id]\noperation join(left: one, right: one)\nresult = join(raw, reference)\n";
    let issues = diagnose(text, Some("sources:\n  raw[id=x]\n"));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, "pipeline");
    assert_eq!(issues[0].line, Some(4));
    assert!(issues[0].message.contains("missing input `right`"));
}

#[test]
fn coverage_error_points_to_the_failing_rule_when_rules_share_a_product() {
    let text = "source raw [site, run]\nrequire raw count>=1 per [site]\nrequire raw count>=2 per [site]\n";
    let issues = diagnose(text, Some("sources:\n  raw[site=A,run=1]\n"));
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
            b"source raw : A<Native> [id]\noperation first(A<X>) -> B<X>\nmiddle = first(raw)\noperation second(B<MNI>) -> C\nfinal = second(middle)\n",
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"line\":5"), "{json}");
    assert!(json.contains("B<Native>"), "{json}");
}
