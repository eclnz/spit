//! Keep the three pipeline steps independent at their module boundaries.
//! This checks direct `crate::module` references, without trying to parse Rust.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Eq, PartialEq)]
enum Layer {
    Shared,
    Compile,
    Inputs,
    Resolve,
    Driver,
}

fn layer(module: &str) -> Layer {
    match module {
        "compile" => Layer::Compile,
        "inputs" => Layer::Inputs,
        "resolver" | "render" => Layer::Resolve,
        "diagnostics" | "editor" | "main" | "cli" | "lib" => Layer::Driver,
        _ => Layer::Shared,
    }
}

fn allowed(from: Layer, to: Layer) -> bool {
    match from {
        Layer::Driver => true,
        Layer::Shared => to == Layer::Shared,
        Layer::Compile => matches!(to, Layer::Shared | Layer::Compile),
        Layer::Inputs => matches!(to, Layer::Shared | Layer::Compile | Layer::Inputs),
        Layer::Resolve => matches!(to, Layer::Shared | Layer::Compile | Layer::Resolve),
    }
}

fn source_files(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            source_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn step_modules_do_not_import_later_steps() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    for file in files {
        let relative = file.strip_prefix(&src).unwrap();
        let module = relative
            .components()
            .next()
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap();
        let module = module.strip_suffix(".rs").unwrap_or(module);
        let from = layer(module);
        if from == Layer::Driver {
            continue;
        }
        let source = fs::read_to_string(&file).unwrap();
        for (line_number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            assert!(
                !code.contains("use crate::{"),
                "{}:{}: use an explicit module path for the architecture check",
                relative.display(),
                line_number + 1
            );
            for reference in code.split("crate::").skip(1) {
                let owner = reference
                    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .next()
                    .unwrap();
                assert!(
                    allowed(from, layer(owner)),
                    "{}:{}: {module} cannot depend on {owner}",
                    relative.display(),
                    line_number + 1
                );
            }
        }
    }
}

/// The most lines a Rust file may have, tests included. A longer file is
/// usually two subjects, and is easier to read as two modules.
const MAX_LINES: usize = 800;

/// Files that were longer than `MAX_LINES` when the limit came in, each
/// with the most lines it may have. A listed file may shrink but not grow,
/// and leaves the list once it is within the limit.
const OVER_LIMIT: &[(&str, usize)] = &[("src/parser/inventory.rs", 804)];

#[test]
fn rust_files_stay_short() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for folder in ["src", "tests", "examples", "benches"] {
        if root.join(folder).is_dir() {
            source_files(&root.join(folder), &mut files);
        }
    }
    let mut problems = Vec::new();
    for file in files {
        let relative = file.strip_prefix(root).unwrap();
        let name = relative.to_str().unwrap().replace('\\', "/");
        let lines = fs::read_to_string(&file).unwrap().lines().count();
        match OVER_LIMIT.iter().find(|(listed, _)| *listed == name) {
            Some(&(_, _)) if lines <= MAX_LINES => problems.push(format!(
                "{name} has {lines} lines, within the limit of {MAX_LINES}: take it out of OVER_LIMIT"
            )),
            Some(&(_, most)) if lines > most => problems.push(format!(
                "{name} has {lines} lines and may not grow past {most}: split it"
            )),
            None if lines > MAX_LINES => problems.push(format!(
                "{name} has {lines} lines, over the limit of {MAX_LINES}: split it"
            )),
            _ => {}
        }
    }
    for (listed, _) in OVER_LIMIT {
        if !root.join(listed).is_file() {
            problems.push(format!("{listed} is in OVER_LIMIT but no longer exists"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
