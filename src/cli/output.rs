//! Writing what a command found: reports, files, and the summary lines.

use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use spit::{
    stage_within, BoundDag, Diagnosis, Diagnostic, FileNames, Pipeline, ResolvedDag, ResolvedInputs,
};

use super::args::{CliArgs, Flag};

/// Diagnostics that have already been printed.
#[derive(Debug)]
pub(crate) struct Reported;

impl std::fmt::Display for Reported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("errors reported")
    }
}

impl Error for Reported {}

/// Print the `.spitdag`, or write it to the `-o` file, a piece at a time.
pub(crate) fn write_spitdag(args: &CliArgs, bound: &BoundDag) -> Result<(), Box<dyn Error>> {
    match args.value(Flag::Output) {
        Some(file) => {
            let written = fs::File::create(&file).and_then(|mut out| bound.write_json(&mut out));
            written.map_err(|reason| format!("cannot write `{file}`: {reason}"))?;
            eprintln!("note: wrote the .spitdag to `{file}`");
        }
        None => bound.write_json(&mut io::stdout().lock())?,
    }
    Ok(())
}

/// Print `text`, or write it to the `-o` file.
pub(crate) fn write_output(args: &CliArgs, text: &str, what: &str) -> Result<(), Box<dyn Error>> {
    match args.value(Flag::Output) {
        Some(file) => {
            fs::write(&file, text).map_err(|reason| format!("cannot write `{file}`: {reason}"))?;
            eprintln!("note: wrote {what} to `{file}`");
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// Sources whose records give their files, which then need no path rule.
pub(crate) fn located(inputs: &ResolvedInputs) -> impl Iterator<Item = &str> {
    inputs
        .inventory
        .artifacts
        .iter()
        .filter(|record| record.path.is_some())
        .map(|record| record.product.as_str())
}

pub(crate) fn is_inputs(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|extension| extension == "spitout")
}

pub(crate) fn is_recipe(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|extension| extension == "spitin")
}

/// How many jobs resolved, per stage when the pipeline has stages.
pub(crate) fn job_count(pipeline: &Pipeline, dag: &ResolvedDag) -> String {
    let total = dag.jobs.len();
    if pipeline.stages.is_empty() {
        return format!("{total} jobs resolved.");
    }
    // Each outermost stage counts the jobs of the stages nested in it.
    let within = |stage: &str| {
        dag.jobs
            .iter()
            .filter(|job| {
                job.stage
                    .as_deref()
                    .is_some_and(|name| stage_within(name, stage))
            })
            .count()
    };
    let mut parts: Vec<_> = pipeline
        .stages
        .iter()
        .filter(|stage| !stage.name.contains('/'))
        .map(|stage| format!("{} in {}", within(&stage.name), stage.name))
        .collect();
    let outside = dag.jobs.iter().filter(|job| job.stage.is_none()).count();
    if outside > 0 {
        parts.push(format!("{outside} outside stages"));
    }
    format!("{total} jobs resolved: {}.", parts.join(", "))
}

pub(crate) fn read_stdin() -> Result<String, String> {
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|reason| format!("cannot read standard input: {reason}"))?;
    Ok(text)
}

/// Read a file, naming it if it cannot be read.
pub(crate) fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|reason| format!("cannot read `{path}`: {reason}"))
}

/// Print every diagnostic, failing if any is an error.
pub(crate) fn report(
    diagnostics: &[Diagnostic],
    text: &str,
    inventory_text: Option<&str>,
    names: FileNames<'_>,
) -> Result<(), Reported> {
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.display_named(text, inventory_text, names));
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(Reported);
    }
    Ok(())
}

/// Print what `diagnosis` found, and pass on what it checked; `warnings`
/// finds its warnings.
pub(crate) fn passed<T>(
    diagnosis: Diagnosis<T>,
    warnings: fn(&T) -> &[Diagnostic],
    text: &str,
    inventory_text: Option<&str>,
    names: FileNames<'_>,
) -> Result<T, Reported> {
    match diagnosis {
        Ok(checked) => {
            report(warnings(&checked), text, inventory_text, names)?;
            Ok(checked)
        }
        Err(all) => {
            report(&all, text, inventory_text, names)?;
            Err(Reported)
        }
    }
}
