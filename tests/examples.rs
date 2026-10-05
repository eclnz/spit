//! Every example must follow every rule SPIT checks, warnings included.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use spit::{diagnose_in, parse_pipeline, parse_source_inventory, resolve, Context};

/// Examples that demonstrate a diagnostic, with exactly what they report.
const EXPECTED: &[(&str, &[&str])] = &[(
    "examples/analytics/analytics_bad_join.spit",
    &["error: line 7: type conflict at `join.accounts` (product `accounts`): variable `K` was inferred as CustomerKey, but now requires AccountKey"],
)];

fn example_pipelines() -> Vec<PathBuf> {
    let mut pending = vec![PathBuf::from("examples")];
    let mut pipelines = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "spit")
            {
                pipelines.push(path);
            }
        }
    }
    pipelines.sort();
    pipelines
}

#[test]
fn worked_patterns_resolve_to_the_documented_jobs() {
    for (name, jobs) in [
        ("archive_revision", 2),
        ("per_group_reference", 3),
        ("model_fit", 2),
        ("ragged_sweep", 17),
        ("cohort", 24),
    ] {
        let base = PathBuf::from("examples/patterns").join(name).join(name);
        let pipeline =
            parse_pipeline(&fs::read_to_string(base.with_extension("spit")).unwrap()).unwrap();
        let inventory =
            parse_source_inventory(&fs::read_to_string(base.with_extension("spitout")).unwrap())
                .unwrap();
        let dag = resolve(&pipeline, &inventory).unwrap();
        assert_eq!(dag.jobs.len(), jobs, "{name}");
    }
}

#[test]
fn mrtrix3_act_inventory_resolves_to_the_documented_jobs() {
    let base = PathBuf::from("examples/commands/mrtrix3_act/mrtrix3_act");
    let path = base.with_extension("spit");
    let text = fs::read_to_string(&path).unwrap();
    let records =
        fs::read_to_string("examples/commands/mrtrix3_act/mrtrix3_mock_data/inputs.spitout")
            .unwrap();
    let found: Vec<_> = diagnose_in(&text, Some(&records), Context::at(&path))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(found.is_empty(), "{found:?}");

    let pipeline = parse_pipeline(&text).unwrap();
    let inventory = parse_source_inventory(&records).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 93);

    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "inputs",
            "examples/commands/mrtrix3_act/mrtrix3_act_discover.spitin",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let saved = records.strip_prefix("root .\n\n").unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), saved);
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
        let found: Vec<_> = diagnose_in(&text, sources.as_deref(), Context::at(&path))
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
