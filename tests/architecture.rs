//! The three steps stay independent in the code: compiling a pipeline,
//! building its inputs, and resolving jobs. Each source file belongs to a
//! step or is shared, and may only use what its step builds on:
//!
//! - shared code (the model, parser, templates) uses no step;
//! - step 1, compile, uses shared code;
//! - step 2, inputs, uses shared code and step 1;
//! - step 3, resolve, uses shared code and step 1, never step 2.
//!
//! `diagnostics.rs`, `main.rs` and `lib.rs` run the steps in order, so they
//! may use all of them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Layer {
    Shared,
    Compile,
    Inputs,
    Resolve,
    /// Runs every step; not checked.
    Driver,
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Shared => "shared code",
            Self::Compile => "step 1 (compile)",
            Self::Inputs => "step 2 (inputs)",
            Self::Resolve => "step 3 (resolve)",
            Self::Driver => "a driver",
        })
    }
}

impl Layer {
    /// What code in this layer may use.
    fn may_use(self, other: Self) -> bool {
        match self {
            Self::Driver => true,
            Self::Shared => other == Self::Shared,
            Self::Compile => matches!(other, Self::Shared | Self::Compile),
            Self::Inputs => matches!(other, Self::Shared | Self::Compile | Self::Inputs),
            Self::Resolve => matches!(other, Self::Shared | Self::Compile | Self::Resolve),
        }
    }
}

/// The layer of a module, named by its path under `src` without the
/// extension: `resolver/matching`, `paths/bind`, `render`.
fn layer_of(module: &str) -> Layer {
    let top = module.split('/').next().unwrap_or(module);
    match (top, module) {
        (_, "paths/rules") => Layer::Compile,
        (_, "paths/bind") => Layer::Resolve,
        ("compile", _) => Layer::Compile,
        ("inputs", _) => Layer::Inputs,
        ("resolver" | "render", _) => Layer::Resolve,
        ("diagnostics" | "main" | "lib", _) => Layer::Driver,
        _ => Layer::Shared,
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

/// `src/paths/mod.rs` as `paths`, `src/paths/bind.rs` as `paths/bind`.
fn module_name(src: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(src).unwrap().with_extension("");
    let name = relative.to_string_lossy().replace('\\', "/");
    name.strip_suffix("/mod").unwrap_or(&name).to_owned()
}

/// The text with comments removed, so that paths in docs are not counted.
fn code(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("//") {
            Some(start) => &line[..start],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Expand a use tree such as `a::{b, c::{d, e}}` into full paths.
fn expand(prefix: &str, tree: &str, paths: &mut Vec<String>) {
    let tree = tree.trim();
    if tree.is_empty() {
        return;
    }
    let join = |rest: &str| {
        if prefix.is_empty() {
            rest.to_owned()
        } else {
            format!("{prefix}::{rest}")
        }
    };
    match tree.find('{') {
        None => {
            let name = tree.split(" as ").next().unwrap().trim();
            paths.push(join(name));
        }
        Some(open) => {
            let head = tree[..open].trim().trim_end_matches("::");
            let inner = &tree[open + 1..tree.rfind('}').expect("closing brace")];
            let base = if head.is_empty() {
                prefix.to_owned()
            } else {
                join(head)
            };
            let mut depth = 0;
            let mut start = 0;
            for (index, character) in inner.char_indices() {
                match character {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    ',' if depth == 0 => {
                        expand(&base, &inner[start..index], paths);
                        start = index + 1;
                    }
                    _ => {}
                }
            }
            expand(&base, &inner[start..], paths);
        }
    }
}

/// Every `crate::` path a file uses, in `use` items or written inline, as
/// the segments after `crate`.
fn crate_paths(code: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut rest = code;
    while let Some(start) = rest.find("crate::") {
        let before = rest[..start].trim_end();
        let after = &rest[start + "crate::".len()..];
        if before.ends_with("use") {
            let end = after.find(';').expect("use item ends with `;`");
            expand("", &after[..end], &mut paths);
            rest = &after[end..];
        } else {
            let end = after
                .find(|character: char| {
                    !(character.is_alphanumeric() || character == '_' || character == ':')
                })
                .unwrap_or(after.len());
            paths.push(after[..end].trim_end_matches(':').to_owned());
            rest = &after[end..];
        }
    }
    paths
}

/// The names a module re-exports, mapped to the module that defines them:
/// `pub use self::bind::{bound_paths}` in `paths` maps `bound_paths` to
/// `paths/bind`, and `pub use resolver::{resolve}` in `lib` maps `resolve`
/// to `resolver`.
fn reexports(module: &str, code: &str) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    let mut rest = code;
    // Whichever comes first, so no re-export is skipped.
    while let Some(start) = [rest.find("pub use "), rest.find("pub(crate) use ")]
        .into_iter()
        .flatten()
        .min()
    {
        let after = &rest[start..];
        let after = &after[after.find("use ").unwrap() + 4..];
        let end = after.find(';').unwrap();
        let mut paths = Vec::new();
        expand("", &after[..end], &mut paths);
        for path in paths {
            let segments: Vec<_> = path
                .split("::")
                .filter(|segment| *segment != "self")
                .collect();
            let (name, owner) = segments.split_last().unwrap();
            let owner = owner.join("/");
            let owner = if module == "lib" {
                owner
            } else {
                format!("{module}/{owner}")
            };
            names.insert((*name).to_owned(), owner);
        }
        rest = &after[end..];
    }
    names
}

/// The module that defines what `path` names, following re-exports.
fn owner(path: &str, roots: &BTreeMap<String, String>, paths: &BTreeMap<String, String>) -> String {
    let segments: Vec<_> = path.split("::").collect();
    let first = segments[0];
    if let Some(owner) = roots.get(first) {
        return owner.clone();
    }
    if first == "paths" {
        if let Some(owner) = segments.get(1).and_then(|name| paths.get(*name)) {
            return owner.clone();
        }
    }
    first.to_owned()
}

#[test]
fn each_step_uses_only_what_it_builds_on() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    files.sort();
    let texts: BTreeMap<_, _> = files
        .iter()
        .map(|file| {
            (
                module_name(&src, file),
                code(&fs::read_to_string(file).unwrap()),
            )
        })
        .collect();
    let roots = reexports("lib", &texts["lib"]);
    let paths = reexports("paths", &texts["paths"]);

    let mut violations = BTreeSet::new();
    let mut edges = BTreeSet::new();
    for (module, text) in &texts {
        let layer = layer_of(module);
        for path in crate_paths(text) {
            let owner = owner(&path, &roots, &paths);
            let used = layer_of(&owner);
            edges.insert((layer, used));
            if !layer.may_use(used) {
                violations.insert(format!(
                    "src/{module}.rs ({layer}) uses `crate::{path}` from `{owner}` ({used})"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "step boundaries crossed:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
    // The check sees the real dependencies, so a pass means something.
    for edge in [
        (Layer::Inputs, Layer::Compile),
        (Layer::Resolve, Layer::Compile),
        (Layer::Driver, Layer::Inputs),
        (Layer::Driver, Layer::Resolve),
    ] {
        assert!(
            edges.contains(&edge),
            "expected {} to use {}",
            edge.0,
            edge.1
        );
    }
}

#[test]
fn a_crossing_is_reported() {
    let mut paths = Vec::new();
    expand(
        "",
        "resolver::{resolve, matching::{expand_step as expand}}",
        &mut paths,
    );
    assert_eq!(
        paths,
        ["resolver::resolve", "resolver::matching::expand_step"]
    );
    let used =
        crate_paths("use crate::inputs::InputSpec;\nlet x = crate::paths::bound_paths(p, d);");
    assert_eq!(used, ["inputs::InputSpec", "paths::bound_paths"]);
    assert!(!layer_of("resolver").may_use(layer_of("inputs")));
    assert!(!layer_of("inputs").may_use(layer_of("paths/bind")));
    assert!(!layer_of("parser/declarations").may_use(layer_of("paths/rules")));
    assert!(layer_of("inputs/discover").may_use(layer_of("compile")));
    let exported = reexports(
        "paths",
        "pub use self::bind::{validate_source_files};\npub(crate) use self::rules::validate_discovery_rule;",
    );
    assert_eq!(exported["validate_discovery_rule"], "paths/rules");
    assert_eq!(exported["validate_source_files"], "paths/bind");
}
