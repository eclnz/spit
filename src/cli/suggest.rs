//! `spit inputs --suggest`: source and path lines for the files no source
//! rule matches, written for the file the user gave.

use std::error::Error;
use std::fmt::Write;

use spit::{SuggestedSource, Suggestions};

use super::load::Loaded;
use super::output::is_recipe;

/// Scan the dataset root and print a suggestion for each group of files no
/// rule matches. For a recipe the path lines are the recipe's, and each new
/// source's line is named as one for its pipeline; for a pipeline both are
/// the pipeline's.
pub(crate) fn suggest(loaded: &Loaded, file: &str) -> Result<(), Box<dyn Error>> {
    let (root, _) = loaded
        .recipe
        .root
        .as_ref()
        .expect("a recipe read from a file names its root, and `--root` gives a pipeline's");
    let suggestions = loaded.recipe.suggest(&loaded.checked.pipeline, root)?;
    if suggestions.sources.is_empty()
        && suggestions.alone.is_empty()
        && suggestions.unfitted.is_empty()
    {
        eprintln!(
            "note: every file under `{}` matches a source rule",
            root.display()
        );
        return Ok(());
    }
    let pipeline = is_recipe(file).then(|| {
        loaded.recipe.pipeline.as_ref().map_or_else(
            || "the pipeline".to_owned(),
            |path| path.display().to_string(),
        )
    });
    print!("{}", render(&suggestions, pipeline.as_deref()));
    Ok(())
}

/// The suggestions as lines to paste. `pipeline` names the recipe's
/// pipeline when the lines are for a recipe.
fn render(suggestions: &Suggestions, pipeline: Option<&str>) -> String {
    let mut text = String::new();
    for (index, source) in suggestions.sources.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        write_source(&mut text, source, pipeline);
    }
    if !suggestions.alone.is_empty() {
        if !suggestions.sources.is_empty() {
            text.push('\n');
        }
        let count = suggestions.alone.len();
        let _ = writeln!(
            text,
            "# {count} file{} like no other, each a source with no dimensions if a step reads it:",
            if count == 1 { "" } else { "s" }
        );
        for file in &suggestions.alone {
            let _ = writeln!(text, "#   {file}");
        }
    }
    if !suggestions.unfitted.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        let names: Vec<_> = suggestions
            .unfitted
            .iter()
            .map(|name| format!("`{name}`"))
            .collect();
        let _ = writeln!(
            text,
            "# no group of files above fits {} alone; give {} a path rule, or rename a source above to {}",
            names.join(", "),
            if names.len() == 1 { "it" } else { "each" },
            if names.len() == 1 { "it" } else { "one of them" },
        );
    }
    text
}

fn write_source(text: &mut String, source: &SuggestedSource, pipeline: Option<&str>) {
    let files = if source.files == 1 { "file" } else { "files" };
    let _ = writeln!(
        text,
        "# {} {files}, such as {}",
        source.files, source.example
    );
    let dimensions = source.dimensions.join(", ");
    if !source.members.is_empty() {
        write_sidecars(text, source, pipeline);
        return;
    }
    match &source.declared {
        Some(declared) => {
            let _ = writeln!(text, "# for `{}`, which the pipeline declares", source.name);
            if *declared != source.dimensions {
                let _ = writeln!(
                    text,
                    "# the pipeline gives it [{}], and these files have [{dimensions}]",
                    declared.join(", ")
                );
            }
        }
        None => {
            let line = format!("source {} [{dimensions}]", source.name);
            match pipeline {
                Some(pipeline) => {
                    let _ = writeln!(text, "# in {pipeline}: {line}");
                }
                None => {
                    let _ = writeln!(text, "{line}");
                }
            }
        }
    }
    write_notes(text, source);
    let _ = writeln!(text, "path {}: {}", source.name, source.rule);
}

/// A `sidecars` block for files that share a stem: in the pipeline with its
/// stem, or in the recipe's pipeline with the stem in the recipe.
fn write_sidecars(text: &mut String, source: &SuggestedSource, pipeline: Option<&str>) {
    let mut block = format!(
        "sidecars {} [{}]:\n",
        source.name,
        source.dimensions.join(", ")
    );
    if pipeline.is_none() {
        let _ = writeln!(block, "    path: {}", source.rule);
    }
    for (name, extension) in &source.members {
        let _ = writeln!(block, "    source {name} {extension}");
    }
    match pipeline {
        Some(pipeline) => {
            let _ = writeln!(text, "# in {pipeline}:");
            for line in block.lines() {
                let _ = writeln!(text, "#   {line}");
            }
            write_notes(text, source);
            let _ = writeln!(text, "path {}: {}", source.name, source.rule);
        }
        None => {
            write_notes(text, source);
            text.push_str(&block);
        }
    }
}

/// What to check before using a suggestion: dimensions no word names, and
/// files the rule matches beyond its own.
fn write_notes(text: &mut String, source: &SuggestedSource) {
    if !source.unnamed.is_empty() {
        let _ = writeln!(
            text,
            "# {} named by place, as no word in the path names them; rename them to say what they hold",
            source.unnamed.join(", "),
        );
    }
    if source.overlaps > 0 {
        let _ = writeln!(
            text,
            "# this rule also matches {} other file{}; a file two rules match is an error, so make one more specific",
            source.overlaps,
            if source.overlaps == 1 { "" } else { "s" }
        );
    }
}
