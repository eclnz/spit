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
        "diagnostics" | "editor" | "main" | "lib" => Layer::Driver,
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
