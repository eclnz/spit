//! Size checks, run on request: `cargo test --release --test scale -- --ignored`.
//! They are ignored by default because a debug build makes them slow. Each
//! bound is loose; it catches a change that makes a stage quadratic or
//! recursive over the whole pipeline, not a slow machine.

use std::time::{Duration, Instant};

use spit::{diagnose, parse_pipeline, parse_source_inventory, resolve, validate_pipeline};

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
