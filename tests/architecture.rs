//! Give every top-level module of `src/` a layer, and keep each module to
//! its own layer and the layers below. This checks direct `crate::module`
//! references, without trying to parse Rust.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A layer of the compiler, from the values every stage shares up to the
/// drivers that run the stages. The Boundaries section of
/// `docs/architecture.md` names each layer's modules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Layer {
    /// Values that know nothing of a pipeline: spans, types, templates, JSON.
    Value,
    /// The data every stage reads, such as `Pipeline` and `ResolvedDag`.
    Model,
    /// Text to a `Pipeline` or an `InputSpec`.
    Parse,
    /// The checks on a parsed pipeline.
    Check,
    /// A recipe and a dataset to a `SourceInventory`.
    Inputs,
    /// Every check of a parsed pipeline and its input rules, in one order.
    Validate,
    /// A pipeline and a `SourceInventory` to a `ResolvedDag`.
    Resolve,
    /// A `ResolvedDag` to a `BoundDag`, and the writers of what the stages make.
    Bind,
    /// The command line, the editor, the diagnostics and the library's API.
    Driver,
}

/// The layer of a top-level module of `src/`, or `None` for a module not
/// given one, which the test reports.
fn layer(module: &str) -> Option<Layer> {
    Some(match module {
        "blob" | "json" | "span" | "template" | "types" => Layer::Value,
        "model" | "shape" => Layer::Model,
        "builtins" | "lower" | "order" | "parser" => Layer::Parse,
        "check" | "command" | "compile" | "paths" | "resources" | "review" => Layer::Check,
        "inputs" => Layer::Inputs,
        "validate" => Layer::Validate,
        "resolver" => Layer::Resolve,
        "render" | "spitdag" => Layer::Bind,
        "cli" | "diagnostics" | "editor" | "lib" | "main" => Layer::Driver,
        _ => return None,
    })
}

/// Whether a module in layer `from` may use one in layer `to`. Inputs and
/// resolution are siblings: each settles its own half of a run from the
/// checked pipeline, so neither uses the other.
fn allowed(from: Layer, to: Layer) -> bool {
    use Layer::*;
    match from {
        Value => matches!(to, Value),
        Model => matches!(to, Value | Model),
        Parse => matches!(to, Value | Model | Parse),
        Check => matches!(to, Value | Model | Parse | Check),
        Inputs => matches!(to, Value | Model | Parse | Check | Inputs),
        Resolve => matches!(to, Value | Model | Parse | Check | Resolve),
        Validate => matches!(to, Value | Model | Parse | Check | Inputs | Validate),
        Bind => matches!(to, Value | Model | Parse | Check | Resolve | Bind),
        Driver => true,
    }
}

/// The edges that break the layers, as `(from, to, reason)`. Each reason
/// names the step of `spit-architecture-plan.md` that removes the edge. The
/// test fails when an edge here no longer breaks a rule, so the list only
/// shrinks.
const BACK_EDGES: &[(&str, &str, &str)] = &[(
    "resolver",
    "spitdag",
    "bind_dag is in resolver/bind.rs, which step 9 moves beside spitdag",
)];

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

/// The top-level modules of `src/`: its files and its folders.
fn top_level_modules(src: &Path) -> BTreeSet<String> {
    fs::read_dir(src)
        .unwrap()
        .map(|entry| {
            let name = entry.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".rs").unwrap_or(&name).to_string()
        })
        .collect()
}

/// Each module's uses of other modules, outside the drivers and unit tests,
/// with the first place each one occurs.
fn edges(src: &Path, modules: &BTreeSet<String>) -> BTreeMap<(String, String), String> {
    let mut files = Vec::new();
    source_files(src, &mut files);
    let mut edges = BTreeMap::new();
    for file in files {
        let relative = file.strip_prefix(src).unwrap();
        let first = relative.components().next().unwrap();
        let first = first.as_os_str().to_str().unwrap();
        let from = first.strip_suffix(".rs").unwrap_or(first);
        if layer(from).is_none_or(|layer| layer == Layer::Driver) {
            continue;
        }
        let source = fs::read_to_string(&file).unwrap();
        for (line_number, line) in source.lines().enumerate() {
            // A file's unit tests come last, and may build their input with
            // any stage, as through the crate's `parse_pipeline`.
            if line.trim() == "#[cfg(test)]" {
                break;
            }
            let code = line.split("//").next().unwrap();
            let place = format!("{}:{}", relative.display(), line_number + 1);
            assert!(
                !code.contains("use crate::{"),
                "{place}: use an explicit module path for the architecture check"
            );
            for reference in code.split("crate::").skip(1) {
                let owner = reference
                    .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .next()
                    .unwrap();
                // A name that is not a module is one `lib.rs` re-exports.
                let to = if modules.contains(owner) {
                    owner
                } else {
                    "lib"
                };
                if to != from {
                    edges
                        .entry((from.to_string(), to.to_string()))
                        .or_insert(place.clone());
                }
            }
        }
    }
    edges
}

/// How an edge breaks the layers.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Break {
    /// It uses a layer above its own, or a sibling.
    Order,
    /// It is one half of two modules in one layer that use each other,
    /// which are one subject in two places. Excusing either half excuses both.
    Cycle,
}

/// How the edge `from -> to` breaks the layers, or `None` when it keeps them
/// or does not occur.
fn broken(from: &str, to: &str, edges: &BTreeMap<(String, String), String>) -> Option<Break> {
    let (Some(from_layer), Some(to_layer)) = (layer(from), layer(to)) else {
        return None;
    };
    if !edges.contains_key(&(from.to_string(), to.to_string())) {
        None
    } else if !allowed(from_layer, to_layer) {
        Some(Break::Order)
    } else if from_layer == to_layer && edges.contains_key(&(to.to_string(), from.to_string())) {
        Some(Break::Cycle)
    } else {
        None
    }
}

#[test]
fn modules_use_only_their_layer_and_below() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let modules = top_level_modules(&src);
    let edges = edges(&src, &modules);
    let excepted: BTreeSet<(&str, &str)> =
        BACK_EDGES.iter().map(|&(from, to, _)| (from, to)).collect();
    let mut problems = Vec::new();
    for module in &modules {
        if layer(module).is_none() {
            problems.push(format!("{module} has no layer: give it one in `layer`"));
        }
    }
    for ((from, to), place) in &edges {
        let Some(how) = broken(from, to, &edges) else {
            continue;
        };
        let (from_layer, to_layer) = (layer(from).unwrap(), layer(to).unwrap());
        let problem = match how {
            Break::Order if excepted.contains(&(from.as_str(), to.as_str())) => continue,
            Break::Order => format!("{to_layer:?} is not below {from_layer:?}"),
            Break::Cycle
                if excepted.contains(&(from.as_str(), to.as_str()))
                    || excepted.contains(&(to.as_str(), from.as_str())) =>
            {
                continue
            }
            Break::Cycle => format!("{to} also uses {from}, in the same layer"),
        };
        problems.push(format!(
            "{place}: {from} ({from_layer:?}) cannot depend on {to} ({to_layer:?}): {problem}"
        ));
    }
    for &(from, to, _) in BACK_EDGES {
        if broken(from, to, &edges).is_none() {
            problems.push(format!(
                "{from} -> {to} no longer breaks the layers: delete it from BACK_EDGES"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The most lines a Rust file may have, tests included. A longer file is
/// usually two subjects, and is easier to read as two modules.
const MAX_LINES: usize = 800;

#[test]
fn rust_files_stay_short() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for folder in ["src", "tests", "examples", "benches"] {
        if root.join(folder).is_dir() {
            source_files(&root.join(folder), &mut files);
        }
    }
    let long: Vec<_> = files
        .iter()
        .filter_map(|file| {
            let lines = fs::read_to_string(file).unwrap().lines().count();
            let name = file.strip_prefix(root).unwrap().display();
            (lines > MAX_LINES).then(|| format!("{name} has {lines} lines: split it"))
        })
        .collect();
    assert!(
        long.is_empty(),
        "over the limit of {MAX_LINES} lines:\n{}",
        long.join("\n")
    );
}
