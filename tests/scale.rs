//! Size checks, run on request: `cargo test --release --test scale -- --ignored`.
//! They are ignored by default because a debug build makes them slow. Each
//! bound is loose; it catches a change that makes a stage quadratic or
//! recursive over the whole pipeline, not a slow machine.

use std::time::{Duration, Instant};

use spit::{parse_pipeline, parse_source_inventory, resolve, validate_pipeline};

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
