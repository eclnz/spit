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
fn a_line_that_fails_still_closes_the_stage_it_is_outdented_from() {
    // The unindented `path:` fails, but indentation alone decides where a
    // stage ends, so it ends stage `outer` all the same, and the indented
    // stage header after it is outside every stage.
    let text = "path: {@product}.txt\nsource raw : Table [id]\noperation copy(table: Table) -> Table\nstage outer:\n    stage first:\n        a = copy(raw)\npath: {@product}.csv\n    stage second:\n        b = copy(a)\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(
        issues
            .iter()
            .map(|issue| (issue.line, issue.message.as_str()))
            .collect::<Vec<_>>(),
        [
            (Some(7), "duplicate default path template"),
            (
                Some(8),
                "a stage header outside every stage starts at the beginning of its line"
            )
        ]
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
        "path: out/{@product}/{id}.txt",
        "operation copy(input: Image) -> Image",
        "command copy: tool {input} {@output}",
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
    let error = spit::parse_input_spec("require [id where raw count>=1\n").unwrap_err();
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

/// Indentation alone decides where a body ends, so a line that fails still
/// ends the body it is outdented from, and a line indented after it is not
/// in the body.
#[test]
fn a_line_that_fails_still_ends_the_body_before_it() {
    let text = "source raw : Image [sub]\noperation step(x: Image) -> Image\noperation body(x: Image) -> (r: Image):\n  r = step(x)\nexclude [sub=1]\nfrob 1\nexclude [sub=1]\n  - raw: x\nstage s:\n  a = step(raw)\nstray line\n  b = step(raw)\n";
    let issues = errors(diagnose(text, None));
    let found: Vec<_> = issues
        .iter()
        .map(|issue| (issue.line, issue.message.as_str()))
        .collect();
    assert_eq!(
        found.iter().map(|(line, _)| *line).collect::<Vec<_>>(),
        [Some(5), Some(6), Some(7), Some(8), Some(11)],
        "{found:?}"
    );
    assert!(found[3].1.starts_with("expected source"), "{found:?}");
}

#[test]
fn calls_to_an_operation_that_failed_to_declare_repeat_its_error() {
    let mut text = String::from("source raw : T [sub]\noperation step(input: T -> T\n");
    for index in 0..50 {
        text += &format!("p{index} = step(raw)\n");
    }
    let issues = errors(diagnose(&text, None));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].line, Some(2));
}

#[test]
fn each_call_to_a_misspelled_operation_is_an_error_of_its_own() {
    let mut text = String::from("source raw : T [sub]\noperation step(input: T) -> T\n");
    for index in 0..50 {
        text += &format!("p{index} = stpe(raw @ vary(sub))\np{index}b = step(raw @ bogus(sub))\n");
    }
    let issues = errors(diagnose(&text, None));
    let lines: Vec<_> = issues.iter().map(|issue| issue.line).collect();
    let expected: Vec<_> = (3..103).map(Some).collect();
    assert_eq!(lines, expected);
}

/// The lines of the errors in `text`, with what each says.
fn error_lines(text: &str) -> Vec<(usize, String)> {
    errors(diagnose(text, None))
        .iter()
        .map(|issue| {
            (
                issue.line.expect("an error has a line"),
                issue.message.clone(),
            )
        })
        .collect()
}

/// The numbers of the lines of `found`.
fn lines_of(found: &[(usize, String)]) -> Vec<usize> {
    found.iter().map(|(line, _)| *line).collect()
}

/// A step of a body that fails to check is left out, as blanking its line
/// would leave it, so the steps after it are checked without it.
#[test]
fn each_bad_step_of_a_body_is_an_error_and_the_operation_keeps_the_rest() {
    let text = "source raw : T [sub]\noperation step(input: T) -> T\noperation body(input: T) -> (out: T):\n    a = nostep(input)\n    out = step(input)\n    b = step(missing)\n    out = step(input)\n    c = step(a)\nq = body(raw)\nr = step(q)\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [4, 6, 7, 8], "{found:?}");
    assert!(found[2].1.contains("already has `out`"), "{found:?}");
    // `a` is made by the step that failed, so the step that reads it fails.
    assert!(found[3].1.contains("reads `a`"), "{found:?}");
}

/// A body whose every step fails has nothing left, which repeats the errors
/// of its steps and is not reported, nor is a call to the operation.
#[test]
fn a_body_with_every_step_failing_is_reported_by_its_steps_alone() {
    let text = "source raw : T [sub]\noperation step(input: T) -> T\noperation body(input: T) -> (out: T):\n    out = nostep(input)\n    x = step(missing)\nq = body(raw)\nr = step(q)\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [4, 5], "{found:?}");
}

/// A rule that repeats one is an error that leaves the first as it was.
#[test]
fn a_repeated_path_extension_or_check_list_is_an_error_of_its_own() {
    let text = "source raw : T [sub]\noperation step(input: T) -> T\npath: a\npath: b\npath raw: c\npath raw: d\next: .x\next: .y\ncheck: nonempty\ncheck: nonempty\nstage s:\n    path: a\n    path: b\n    ext: .x\n    ext: .y\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [4, 6, 8, 10, 13, 15], "{found:?}");
}

/// The first line of a stage fixes how far its lines are indented, even
/// when it fails, and the lines after are read against that.
#[test]
fn the_first_line_of_a_stage_sets_its_indentation_even_when_it_fails() {
    let text = "source raw : T [sub]\noperation step(input: T) -> T\nstage s:\n    a = nostep(raw)\n      b = step(raw)\n    c = step(raw)\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [4, 5], "{found:?}");
    assert!(found[1].1.contains("indented differently"), "{found:?}");
}

/// A line that fails still closes the stage it is outdented from, so a
/// line indented after it is at the top level, where any indentation is
/// allowed.
#[test]
fn a_line_that_fails_still_closes_the_stage_before_it() {
    let text = "source raw : T [sub]\noperation step(input: T) -> T\nstage s:\n    a = step(raw)\np = nostep(raw)\n    b = step(raw)\nq = step(raw)\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [5], "{found:?}");
}

/// A call whose arguments clash with what its operation's body reads fails
/// without recording the call, so each call is an error of its own.
#[test]
fn each_call_whose_selectors_clash_with_its_body_is_an_error_of_its_own() {
    let text = "source raw : T [sub]\noperation step(input: T) -> T\noperation inner(input: T) -> (out: T):\n    out = step(input @ where(sub=1))\noperation outer(input: T) -> (out: T):\n    out = inner(input @ where(sub=1))\nm = outer(raw @ where(sub=2))\nn = outer(raw @ where(sub=2))\nk = outer(raw)\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [7, 8], "{found:?}");
}

/// A stage header that fails still opens its block: its lines are read in
/// the stage around it, so a stage nested in them is where it belongs, and
/// an error among them is found in the same pass.
#[test]
fn a_stage_header_that_fails_keeps_its_lines() {
    let text = "source a : A [s]\noperation f(x: A) -> A\nstage outer:\n    stage pre\n        b = f(a)\n        stage inner:\n            c = f(b)\n        d = nope(c)\n    e = f(d)\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [4, 8], "{found:?}");
    assert!(
        found[0].1.starts_with("expected `stage name:`"),
        "{found:?}"
    );
}

/// The steps of a body whose header fails are read where the body is, as
/// ordinary statements: a call to the operation repeats the header's error
/// and is not reported, and each step is still read.
#[test]
fn the_steps_of_a_body_whose_header_fails_are_read_as_ordinary_statements() {
    let text = "source a : A [s]\nsource x : A [s]\noperation f(x: A) -> A\noperation h(x A) -> A:\n    y = f(x)\n    z = bad(\nc = h(a)\nd = f(c)\n";
    let found = error_lines(text);
    assert_eq!(lines_of(&found), [4, 6], "{found:?}");
}
