//! Fixtures written as one text: a pipeline, then its records under
//! `sources:` or `contexts:`. SPIT reads the two from separate files, so
//! these split them first.

#![allow(dead_code)]

use std::process::{Command, Output};

use spit::{
    parse_pipeline, parse_source_inventory, Diagnostic, ParseError, Pipeline, ResolvedDag,
    SourceInventory,
};

/// The pipeline part of `text`, and its records, if any. The pipeline keeps
/// its line numbers; the records are numbered from their own first line.
pub fn split(text: &str) -> (String, Option<String>) {
    let lines: Vec<_> = text.lines().collect();
    let start = lines.iter().position(|line| {
        let line = line.trim();
        line == "sources:"
            || line == "contexts:"
            || (line.starts_with("contexts ") && line.ends_with(':'))
    });
    match start {
        None => (text.to_owned(), None),
        Some(start) => (
            lines[..start].join("\n") + "\n",
            Some(lines[start..].join("\n") + "\n"),
        ),
    }
}

/// The pipeline and records of a fixture, each parsed as its own file is.
pub fn parse_fixture(text: &str) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    let (pipeline, records) = split(text);
    let records = records
        .map(|records| parse_source_inventory(&records))
        .transpose()?;
    Ok((parse_pipeline(&pipeline)?, records))
}

/// Whether `line` is a `discover`, `require` or `skip` rule.
fn is_rule(line: &str) -> bool {
    let line = line.trim_start();
    line.trim_end() == "constraints:"
        || ["discover ", "require ", "skip "]
            .iter()
            .any(|keyword| line.starts_with(keyword))
}

/// The pipeline part of `text` with its rule lines blanked, so it keeps its
/// line numbers, and those rules as the text of a recipe.
pub fn split_rules(text: &str) -> (String, String) {
    let mut pipeline = String::new();
    let mut recipe = String::new();
    for line in text.lines() {
        if is_rule(line) {
            if line.trim() != "constraints:" {
                recipe.push_str(line.trim_start());
                recipe.push('\n');
            }
        } else {
            pipeline.push_str(line);
        }
        pipeline.push('\n');
    }
    (pipeline, recipe)
}

/// A fixture's pipeline, its rules as a recipe, and its records.
pub fn parse_with_rules(
    text: &str,
) -> Result<(Pipeline, spit::InputSpec, Option<SourceInventory>), ParseError> {
    let (pipeline, records) = split(text);
    let (pipeline, recipe) = split_rules(&pipeline);
    let (pipeline, records) = parse_fixture(&(pipeline + &records.unwrap_or_default()))?;
    Ok((pipeline, spit::parse_input_spec(&recipe)?, records))
}

/// As [`parse_fixture`], for a pipeline at `path`, whose imports resolve
/// from its folder.
pub fn parse_fixture_at(
    text: &str,
    path: &std::path::Path,
) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    let (pipeline, records) = split(text);
    let records = records
        .map(|records| parse_source_inventory(&records))
        .transpose()?;
    Ok((spit::parse_pipeline_at(&pipeline, path)?, records))
}

/// A folder under the system's temporary directory, with empty files at the
/// paths given, removed when dropped. Each is new, whichever test makes it.
pub struct Tree(pub std::path::PathBuf);

impl Tree {
    pub fn new(name: &str, files: &[&str]) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "spit-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let tree = Self(root);
        for file in files {
            tree.write(file, "");
        }
        tree
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }

    /// Write `contents` to `name` in the tree, making its folders.
    pub fn write(&self, name: &str, contents: &str) -> std::path::PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The jobs with their bound paths, or the binding error as text.
pub fn bound(pipeline: &spit::Pipeline, dag: &spit::ResolvedDag) -> Result<String, String> {
    let bound = spit::bind_dag(pipeline, dag).map_err(|error| error.to_string())?;
    Ok(spit::render_bound_dag(
        &bound,
        spit::View {
            paths: true,
            ..spit::View::default()
        },
    ))
}

/// Only the diagnostics that are errors.
pub fn errors(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

/// Every job output, as `product[entities]`, in job order.
pub fn outputs(dag: &ResolvedDag) -> Vec<String> {
    dag.jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .map(|&output| dag.artifact(output).to_string())
        .collect()
}

/// Each diagnostic as the command line prints it, without columns.
pub fn rendered(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics.iter().map(ToString::to_string).collect()
}

/// Command output as text.
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Run the `spit` binary with `args`.
pub fn spit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .output()
        .unwrap()
}
