//! Damaged input must never panic. The parsers slice text by byte offset, so
//! this inserts, deletes and truncates characters, multi-byte ones among
//! them, at every position of the example files and feeds the result to each
//! entry point. Errors are fine; panics are not.

use std::fs;
use std::panic::{self, AssertUnwindSafe};

use spit::{
    diagnose, diagnose_recipe_against, parse_input_spec, parse_pipeline, parse_source_inventory,
    Pipeline,
};

const INSERTS: [&str; 18] = [
    "é", "日", "😀", "\u{feff}", "#", "\"", "'", "\\", "{", "}", "(", ")", "[", "]", "<", ">", "@",
    ",",
];

fn example(name: &str) -> String {
    fs::read_to_string(format!("examples/basic/{name}")).unwrap()
}

/// `text` with `insert` at each char boundary, with each char removed, and
/// truncated at each char boundary.
fn mutations(text: &str) -> impl Iterator<Item = String> + '_ {
    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()]);
    boundaries.flat_map(move |at| {
        let inserted = INSERTS.iter().map(move |insert| {
            let mut mutated = text.to_owned();
            mutated.insert_str(at, insert);
            mutated
        });
        let mut removed = text.to_owned();
        let deleted = (at < text.len()).then(|| {
            removed.remove(at);
            removed
        });
        inserted.chain(deleted).chain([text[..at].to_owned()])
    })
}

/// Run `entry` on every mutation of `text`, returning the inputs that
/// panicked.
fn panics(text: &str, entry: impl Fn(&str)) -> Vec<String> {
    mutations(text)
        .filter(|mutated| panic::catch_unwind(AssertUnwindSafe(|| entry(mutated))).is_err())
        .take(5)
        .collect()
}

#[test]
fn a_damaged_pipeline_never_panics() {
    let failures = panics(&example("basic.spit"), |text| {
        let _ = parse_pipeline(text);
        let _ = diagnose(text, None);
    });
    assert!(failures.is_empty(), "panicked on: {failures:#?}");
}

#[test]
fn a_damaged_recipe_never_panics() {
    let failures = panics(&example("basic.spitin"), |text| {
        let _ = parse_input_spec(text);
        let _ = diagnose_recipe_against(text, &Pipeline::default());
    });
    assert!(failures.is_empty(), "panicked on: {failures:#?}");
}

#[test]
fn damaged_records_never_panic() {
    let pipeline = example("basic.spit");
    let failures = panics(&example("basic.spitout"), |text| {
        let _ = parse_source_inventory(text);
        let _ = diagnose(&pipeline, Some(text));
    });
    assert!(failures.is_empty(), "panicked on: {failures:#?}");
}
