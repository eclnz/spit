//! Syntax errors: each independent one is reported on its own line, however
//! the text was damaged, and without cascading into later lines.

use std::collections::{BTreeMap, BTreeSet};

use spit::{diagnose, Diagnostic, DiagnosticSource};

/// The errors among `diagnostics`.
fn errors(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
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
fn an_error_that_closes_a_stage_is_reported_alone() {
    // The unindented `path:` ends stage `outer`, which would leave the
    // indented stage header after it outside every stage. Only the first
    // error is real: without that line, the header is where it belongs.
    let text = "path: {product}.txt\nsource raw : Table [id]\noperation copy(Table) -> Table\nstage outer:\n    stage first:\n        a = copy(raw)\npath: {product}.csv\n    stage second:\n        b = copy(a)\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(
        issues
            .iter()
            .map(|issue| (issue.line, issue.message.as_str()))
            .collect::<Vec<_>>(),
        [(Some(7), "duplicate default path template")]
    );
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
