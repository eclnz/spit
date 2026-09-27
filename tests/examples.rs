//! Every example must follow every rule SPIT checks, warnings included.

use std::fs;
use std::path::PathBuf;

use spit::diagnose_at;

/// Examples that demonstrate a diagnostic, with exactly what they report.
const EXPECTED: &[(&str, &[&str])] = &[(
    "examples/analytics/analytics_bad_join.spit",
    &["error: line 12: type conflict at `join.input2` (product `accounts`): variable `K` was inferred as CustomerKey, but now requires AccountKey"],
)];

fn example_pipelines() -> Vec<PathBuf> {
    let mut pipelines: Vec<_> = fs::read_dir("examples")
        .unwrap()
        .flat_map(|group| fs::read_dir(group.unwrap().path()).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "spit")
        })
        .collect();
    pipelines.sort();
    pipelines
}

#[test]
fn examples_have_no_diagnostics() {
    let pipelines = example_pipelines();
    assert!(pipelines.len() >= 10, "{pipelines:?}");
    for path in pipelines {
        let text = fs::read_to_string(&path).unwrap();
        // A pipeline's inventory, when separate, sits beside it. One kept
        // inline is checked as it is; a separate copy would replace it.
        let inline = text.lines().any(|line| line.trim() == "sources:");
        let sources = fs::read_to_string(path.with_extension("sources"))
            .ok()
            .filter(|_| !inline);
        let found: Vec<_> = diagnose_at(&text, sources.as_deref(), &path)
            .iter()
            .map(ToString::to_string)
            .collect();
        let expected = EXPECTED
            .iter()
            .find(|(example, _)| path.ends_with(example))
            .map_or(&[][..], |(_, diagnostics)| diagnostics);
        assert_eq!(found, expected, "{}", path.display());
    }
}
