//! Size checks, run on request: `cargo test --release --test scale -- --ignored`.
//! They are ignored by default because a debug build makes them slow. Each
//! bound is loose; it catches a change that makes a stage quadratic or
//! recursive over the whole pipeline, not a slow machine.

use std::time::{Duration, Instant};

mod support;

use spit::{
    diagnose, diagnose_in, parse_pipeline, parse_source_inventory, render_diagnostics_json,
    resolve, validate_pipeline, Context, FileNames, SourceLines,
};

/// A chain of `steps` steps, each reading the one before.
fn chain(steps: usize) -> String {
    let mut text = String::from("source p0 : T [sub]\noperation step(input: T) -> T\n");
    for index in 1..=steps {
        text += &format!("p{index} = step(p{})\n", index - 1);
    }
    text
}

#[test]
#[ignore = "slow in a debug build; run with --release"]
fn a_long_chain_of_steps_compiles_without_overflowing_the_stack() {
    let start = Instant::now();
    let pipeline = parse_pipeline(&chain(200_000)).unwrap();
    validate_pipeline(&pipeline).unwrap();
    assert!(start.elapsed() < Duration::from_secs(30));
}

#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_sources_resolve_in_reasonable_time() {
    let pipeline =
        parse_pipeline("source raw : T [sub]\noperation f(input: T) -> U\nout = f(raw)\n").unwrap();
    let mut records = String::from("sources:\n");
    for index in 0..40_000 {
        records += &format!("    raw[sub={index}]\n");
    }
    let inventory = parse_source_inventory(&records).unwrap();
    let start = Instant::now();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 40_000);
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// A source, an operation, and `steps` steps written by `step`, each given
/// its number.
fn bad_steps(steps: usize, step: impl Fn(usize) -> String) -> String {
    let mut text = String::from("source raw : T [sub]\noperation step(input: T) -> T\n");
    for index in 0..steps {
        text += &step(index);
        text.push('\n');
    }
    text
}

/// Error recovery blanks the lines it reports and reads on, so each bad line
/// must cost its own work, not another reading of the whole file: the editor
/// takes this path on every change. 20,000 bad lines took minutes when each
/// was read again; they take well under a second now.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_bad_selectors_are_reported_in_reasonable_time() {
    let text = bad_steps(20_000, |index| format!("p{index} = step(raw @ bogus(sub))"));
    let start = Instant::now();
    let issues = diagnose(&text, None);
    assert_eq!(issues.len(), 20_000);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_calls_to_a_misspelled_operation_are_reported_in_reasonable_time() {
    let text = bad_steps(20_000, |index| format!("p{index} = stpe(raw)"));
    let start = Instant::now();
    let issues = diagnose(&text, None);
    assert_eq!(issues.len(), 20_000);
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// Calls to an operation whose declaration failed repeat that error, so
/// they are left out, and finding that out must not read the earlier errors
/// again for each call.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_calls_to_an_operation_that_failed_to_declare_are_left_out_in_reasonable_time() {
    let mut text = String::from("source raw : T [sub]\noperation step(input: T -> T\n");
    for index in 0..20_000 {
        text += &format!("p{index} = step(raw)\n");
    }
    let start = Instant::now();
    let issues = diagnose(&text, None);
    assert_eq!(issues.len(), 1);
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// The best of a few runs of `work`, which steadies a timing against noise.
fn best_of(runs: usize, mut work: impl FnMut()) -> Duration {
    (0..runs)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .min()
        .unwrap_or_default()
}

/// Rendering finds each diagnostic's line by its number, not by counting
/// from the top of the file, so it costs the diagnostics, not the diagnostics
/// times the length of the file. This runs with the normal tests: it compares
/// four times the bad lines to one, so a slow machine does not fail it, and
/// quadratic rendering, which takes sixteen times as long, does.
#[test]
fn rendering_diagnostics_costs_the_diagnostics_not_the_length_of_the_file() {
    let small = bad_steps(2_000, |index| format!("p{index} = step(raw @ bogus(sub))"));
    let large = bad_steps(8_000, |index| format!("p{index} = step(raw @ bogus(sub))"));
    let (few, many) = (diagnose(&small, None), diagnose(&large, None));
    assert_eq!((few.len(), many.len()), (2_000, 8_000));
    let render = |diagnostics: &[spit::Diagnostic], text: &str| {
        let json = render_diagnostics_json(diagnostics, text, None);
        let lines = SourceLines::new(diagnostics, text, None);
        let shown: usize = diagnostics
            .iter()
            .map(|diagnostic| {
                diagnostic
                    .display_with(&lines, FileNames::default())
                    .to_string()
                    .len()
            })
            .sum();
        assert!(json.len() > shown);
    };
    let (one, four) = (
        best_of(5, || render(&few, &small)),
        best_of(5, || render(&many, &large)),
    );
    // Linear is about 4; quadratic is about 16. Below a millisecond a ratio
    // is only noise.
    assert!(
        four < Duration::from_millis(50) || four < one * 10,
        "{one:?} for 2,000 bad lines, {four:?} for 8,000"
    );
}

/// A source and an operation `step`, as `bad_steps` starts, then `more`.
fn with_step(more: &str) -> String {
    bad_steps(0, |_| String::new()) + more
}

/// A pipeline with one operation `body` that has a step that is good and
/// `steps` that each fail to check.
fn body_with_bad_steps(steps: usize) -> String {
    let mut text = with_step("operation body(input: T) -> (out: T):\n    out = step(input)\n");
    for index in 0..steps {
        text += &format!("    x{index} = nostep(input)\n");
    }
    text
}

/// The time `diagnose` takes over `text`, the best of three.
fn diagnosing(text: &str) -> Duration {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            let issues = diagnose(text, None);
            assert!(!issues.is_empty());
            start.elapsed()
        })
        .min()
        .expect("three runs")
}

/// A step of an operation's body that fails is left out and the steps after
/// it are checked, so a body with many bad steps is read once, not once for
/// each. Four times the steps must take about four times as long, where it
/// took sixteen times as long when each was read again, which a small file
/// shows.
#[test]
fn bad_steps_in_a_body_cost_their_own_work_and_not_a_reading_of_the_file_each() {
    let small = diagnosing(&body_with_bad_steps(500));
    let large = diagnosing(&body_with_bad_steps(2_000));
    assert!(
        large < small * 8 + Duration::from_millis(20),
        "500 bad steps took {small:?}, 2,000 took {large:?}"
    );
}

#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_bad_steps_in_an_operation_body_are_reported_in_reasonable_time() {
    let text = body_with_bad_steps(20_000);
    let start = Instant::now();
    let issues = diagnose(&text, None);
    assert_eq!(issues.len(), 20_000);
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// A body whose every step fails is left with none, which repeats their
/// errors, so it is not reported.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn a_body_with_every_step_failing_is_reported_in_reasonable_time() {
    let mut text = with_step("operation body(input: T) -> (out: T):\n");
    for index in 0..20_000 {
        text += &format!("    x{index} = nostep(input)\n");
    }
    let start = Instant::now();
    let issues = diagnose(&text, None);
    assert_eq!(issues.len(), 20_000);
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// A rule that repeats one fails without changing what it repeats.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_repeated_path_extension_and_check_lines_are_reported_in_reasonable_time() {
    for first in ["path: a", "path raw: a", "ext: .a", "check: nonempty"] {
        let text =
            format!("source raw : T [sub]\n{first}\n") + &format!("{first}\n").repeat(20_000);
        let start = Instant::now();
        let issues = diagnose(&text, None);
        assert_eq!(issues.len(), 20_000, "{first}");
        assert!(start.elapsed() < Duration::from_secs(10), "{first}");
    }
}

/// An import that conflicts with what a file already has fails before it
/// changes anything.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_conflicting_imports_are_reported_in_reasonable_time() {
    let tree = support::Tree::new("scale-imports", &[]);
    tree.write("parts.spit", "source cleaned : T [sub]\n");
    let text =
        "source raw : T [sub]\nuse parts.spit\n".to_owned() + &"use parts.spit\n".repeat(20_000);
    let main = tree.write("main.spit", &text);
    let start = Instant::now();
    let issues = diagnose_in(&text, None, Context::at(&main));
    assert_eq!(issues.len(), 20_000);
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// A call whose selectors clash with its operation's body is not recorded.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_calls_that_clash_with_their_operations_body_are_reported_in_reasonable_time() {
    let mut text = with_step(
        "operation inner(input: T) -> (out: T):\n    out = step(input @ where(sub=1))\noperation outer(input: T) -> (out: T):\n    out = inner(input @ where(sub=1))\n",
    );
    for index in 0..20_000 {
        text += &format!("m{index} = outer(raw @ where(sub=2))\n");
    }
    let start = Instant::now();
    let issues = diagnose(&text, None);
    assert_eq!(issues.len(), 20_000);
    assert!(start.elapsed() < Duration::from_secs(10));
}

/// The first line of a stage fixes the indentation of its lines, and a line
/// that closes a stage may leave it open for the lines after: each stage
/// with a bad line of either kind must cost its own work.
#[test]
#[ignore = "slow in a debug build; run with --release"]
fn many_stages_with_a_bad_first_line_or_a_bad_closing_line_are_reported_in_reasonable_time() {
    let mut first = with_step("");
    let mut closing = with_step("");
    for index in 0..20_000 {
        first += &format!("stage s{index}:\n    a = nostep(raw)\n    b = step(raw)\n");
        closing += &format!("stage s{index}:\n    a = step(raw)\np{index} = nostep(raw)\n");
    }
    for (kind, text) in [("first", first), ("closing", closing)] {
        let start = Instant::now();
        let issues = diagnose(&text, None);
        assert_eq!(issues.len(), 20_000, "{kind}");
        assert!(start.elapsed() < Duration::from_secs(10), "{kind}");
    }
}
