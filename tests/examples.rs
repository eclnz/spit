//! Every example must follow every rule SPIT checks, warnings included.

use std::fs;
use std::path::{Path, PathBuf};
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
    for (name, jobs) in [("ragged_sweep", 17), ("cohort", 24)] {
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

/// Every recipe in the examples, found by walking the folder.
fn example_recipes() -> Vec<PathBuf> {
    let mut pending = vec![PathBuf::from("examples")];
    let mut recipes = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "spitin")
            {
                recipes.push(path);
            }
        }
    }
    recipes.sort();
    recipes
}

/// What `spit dag` prints for `arguments`, which must succeed.
fn dag_output(arguments: &[&Path]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .arg("dag")
        .args(arguments)
        .args(["--paths", "--commands"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_recipe_and_the_spitout_beside_it_settle_the_same_jobs() {
    let recipes = example_recipes();
    assert!(recipes.len() >= 10, "{recipes:?}");
    for recipe in recipes {
        let spitout = recipe.with_extension("spitout");
        let text = fs::read_to_string(&recipe).unwrap();
        let named = text
            .lines()
            .find_map(|line| line.strip_prefix("pipeline "))
            .unwrap_or_else(|| panic!("{} names no pipeline", recipe.display()));
        let pipeline = recipe.with_file_name(named.trim());
        let from_recipe = dag_output(&[&recipe]);
        let from_spitout = dag_output(&[&pipeline, &spitout]);
        assert!(
            from_recipe == from_spitout,
            "{} and {} settle different jobs",
            recipe.display(),
            spitout.display()
        );
    }
}
