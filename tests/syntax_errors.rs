//! Syntax errors: each independent one is reported on its own line, however
//! the text was damaged, and without cascading into later lines.

mod support;

use support::errors;

use std::collections::{BTreeMap, BTreeSet};

use spit::{diagnose, DiagnosticSource};

#[test]
fn reports_syntax_line_from_unsaved_text() {
    let text =
        "source raw [id]\noperation copy(input)\nresult = copy(raw @ vary(id) @ vary(extra))\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].source, DiagnosticSource::Pipeline);
    assert_eq!(issues[0].line, Some(3));
}

#[test]
fn reports_independent_syntax_errors_across_a_pipeline() {
    let text =
        "source raw [id]\nthis is invalid\noperation copy(input)\nalso invalid\nresult = copy(raw)\n";
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
    let text = "path: {product}.txt\nsource raw : Table [id]\noperation copy(table: Table) -> Table\nstage outer:\n    stage first:\n        a = copy(raw)\npath: {product}.csv\n    stage second:\n        b = copy(a)\n";
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
fn seeded_deletions_report_every_damaged_pipeline_line() {
    let mut original = vec![
        "source raw [id]".to_owned(),
        "operation copy(input)".to_owned(),
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
    let pipeline: Vec<String> = [
        "source raw : Image [id]",
        "path: out/{product}/{id}.txt",
        "operation copy(input: Image) -> Image",
        "command copy: tool {input} {output}",
        "result = copy(raw)",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let inventory = ["sources:", "  raw[id=A]"].map(str::to_owned);
    let diagnose_both = |pipeline: &[String], inventory: &[String]| {
        errors(diagnose(
            &pipeline.join("\n"),
            Some(&(inventory.join("\n") + "\n")),
        ))
    };
    assert!(diagnose_both(&pipeline, &inventory).is_empty());
    // A line of the pipeline, or of the inventory, with one piece deleted.
    let cases = [
        (
            DiagnosticSource::Pipeline,
            0,
            "]",
            "closing `]` in product declaration",
        ),
        (DiagnosticSource::Pipeline, 0, "Image", "expected type name"),
        (
            DiagnosticSource::Pipeline,
            1,
            ":",
            "expected `:` after path product",
        ),
        (
            DiagnosticSource::Pipeline,
            2,
            "(",
            "expected `(` after operation name",
        ),
        (
            DiagnosticSource::Pipeline,
            2,
            "->",
            "expected `->` before operation output type",
        ),
        (DiagnosticSource::Pipeline, 3, ":", "expected command"),
        (
            DiagnosticSource::Pipeline,
            4,
            "=",
            "expected `=` before operation call",
        ),
        (DiagnosticSource::Pipeline, 4, ")", "expected closing `)`"),
        (
            DiagnosticSource::Inventory,
            1,
            "=",
            "expected `dimension=value`",
        ),
        (
            DiagnosticSource::Inventory,
            1,
            "]",
            "expected closing `]` in source artifact",
        ),
    ];
    for (source, line, deleted, expected) in cases {
        let (mut damaged_pipeline, mut damaged_inventory) = (pipeline.clone(), inventory.clone());
        let lines = match source {
            DiagnosticSource::Pipeline => &mut damaged_pipeline[..],
            DiagnosticSource::Inventory => &mut damaged_inventory[..],
        };
        lines[line] = lines[line].replacen(deleted, "", 1);
        let issues = diagnose_both(&damaged_pipeline, &damaged_inventory);
        assert_eq!(
            issues.len(),
            1,
            "deleting {deleted:?} on {source} line {}: {issues:?}",
            line + 1
        );
        assert_eq!((issues[0].source, issues[0].line), (source, Some(line + 1)));
        assert!(
            issues[0].message.contains(expected),
            "deleting {deleted:?} on {source} line {}: {}",
            line + 1,
            issues[0].message
        );
    }
    // A recipe's rules are parsed the same way.
    let error = spit::parse_input_spec("require raw count>=1 per [id\n").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("closing `]` in constraint dimensions"),
        "{error}"
    );
}

fn next_random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn records_in_a_pipeline_are_one_error_however_many_lines() {
    let text =
        "source x [s]\nsources:\n    x[s=1]\n    x[s=2\ncontexts:\n    [s=3]\noperation f(one\n";
    let issues = errors(diagnose(text, None));
    let found: Vec<_> = issues
        .iter()
        .map(|issue| (issue.line, issue.message.as_str()))
        .collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].0, Some(2));
    assert!(found[0].1.contains("belong in a .spitout"), "{found:?}");
    assert_eq!(found[1].0, Some(7));
}
