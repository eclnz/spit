//! Bash script generation for resolved DAGs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::path::Path;

use crate::model::ArtifactKey;
use crate::model::{ArtifactInstance, Cardinality, Job, OperationDef, Pipeline, ResolvedDag};
use crate::paths::{bound_paths, inspect_paths, PathError};
use crate::template::{parse_template, Part};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BashError(pub String);

impl fmt::Display for BashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BashError {}

impl From<PathError> for BashError {
    fn from(error: PathError) -> Self {
        Self(error.0)
    }
}

fn error(message: impl Into<String>) -> BashError {
    BashError(message.into())
}

/// Generate a script for the concrete jobs already selected by `resolve`.
/// Each artifact path is derived from its product and entity bindings.
pub fn render_bash(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<String, BashError> {
    inspect_paths(pipeline)?.validate(false)?;
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let mut commands = BTreeMap::new();
    for command in &pipeline.commands {
        let Some(operation) = operations.get(command.operation.as_str()) else {
            return Err(error(format!(
                "command refers to unknown operation `{}`",
                command.operation
            )));
        };
        if operation.inputs.iter().any(|port| port.name == "output") {
            return Err(error(format!(
                "operation `{}` has an input port named `output`, which shadows `{{output}}`",
                operation.name
            )));
        }
        if commands
            .insert(command.operation.as_str(), command.template.as_str())
            .is_some()
        {
            return Err(error(format!(
                "duplicate command for operation `{}`",
                command.operation
            )));
        }
    }
    let outputs: BTreeSet<_> = dag.jobs.iter().map(|job| job.output.key()).collect();
    let paths = bound_paths(pipeline, dag)?;

    let mut script =
        String::from("#!/usr/bin/env bash\nset -euo pipefail\nSPIT_ROOT=\"${SPIT_ROOT:-.}\"\n\n");
    script.push_str("spit_require() {\n  if [[ ! -e \"$1\" ]]; then\n    printf 'missing artifact: %s\\n' \"$1\" >&2\n    exit 1\n  fi\n}\n\n");

    for (identity, relative) in &paths {
        if !outputs.contains(identity) {
            writeln!(script, "spit_require {}", shell_path(relative)).unwrap();
        }
    }
    if paths.keys().any(|identity| !outputs.contains(identity)) {
        script.push('\n');
    }
    for job in &dag.jobs {
        let operation = operations.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
        let template = commands.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "no command defined for operation `{}`",
                job.operation
            ))
        })?;
        let output_path = &paths[&job.output.key()];
        let parent = Path::new(output_path)
            .parent()
            .and_then(|path| path.to_str())
            .filter(|path| !path.is_empty())
            .unwrap_or(".");
        writeln!(script, "# Job {}: {}", job.id, job.operation).unwrap();
        writeln!(script, "mkdir -p -- {}", shell_path(parent)).unwrap();
        writeln!(
            script,
            "{}",
            render_command(template, operation, job, &paths)?
        )
        .unwrap();
        writeln!(script, "spit_require {}\n", shell_path(output_path)).unwrap();
    }
    Ok(script)
}

fn render_command(
    template: &str,
    operation: &OperationDef,
    job: &Job,
    paths: &BTreeMap<ArtifactKey, String>,
) -> Result<String, BashError> {
    let words = split_words(template)?;
    if words.is_empty() {
        return Err(error(format!("command for `{}` is empty", operation.name)));
    }
    let mut args = Vec::new();
    let mut uses_output = false;
    for word in words {
        let parts = parse_template(&word).map_err(BashError)?;
        if let [Part::Placeholder(name)] = parts.as_slice() {
            if let Some(artifacts) = many_input(operation, job, name) {
                for artifact in artifacts {
                    args.push(shell_path(&paths[&artifact.key()]));
                }
                continue;
            }
        }
        let mut arg = String::new();
        for part in parts {
            match part {
                Part::Literal(value) => arg.push_str(&shell_quote(&value)),
                Part::Placeholder(name) if name == "output" => {
                    uses_output = true;
                    arg.push_str(&shell_path(&paths[&job.output.key()]));
                }
                Part::Placeholder(name) => {
                    if name == "inputs" && many_input(operation, job, &name).is_some() {
                        return Err(error(
                            "many input `{inputs}` must be a complete command argument",
                        ));
                    }
                    let index = operation
                        .inputs
                        .iter()
                        .position(|port| port.name == name)
                        .ok_or_else(|| {
                            error(format!(
                                "command for `{}` uses unknown placeholder `{{{name}}}`",
                                operation.name
                            ))
                        })?;
                    if operation.inputs[index].cardinality == Cardinality::Many {
                        return Err(error(format!(
                            "many input `{{{name}}}` must be a complete command argument"
                        )));
                    }
                    let artifact = job
                        .inputs
                        .get(index)
                        .ok_or_else(|| error(format!("job {} lacks input `{name}`", job.id)))?;
                    arg.push_str(&shell_path(&paths[&artifact.key()]));
                }
            }
        }
        args.push(arg);
    }
    if !uses_output {
        return Err(error(format!(
            "command for `{}` must use `{{output}}`",
            operation.name
        )));
    }
    Ok(args.join(" "))
}

fn many_input<'a>(
    operation: &OperationDef,
    job: &'a Job,
    name: &str,
) -> Option<&'a [ArtifactInstance]> {
    if operation.inputs.len() == 1
        && operation.inputs[0].cardinality == Cardinality::Many
        && (name == operation.inputs[0].name || name == "inputs")
    {
        Some(&job.inputs)
    } else {
        None
    }
}

fn shell_path(relative: &str) -> String {
    format!("\"$SPIT_ROOT\"/{}", shell_quote(relative))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn split_words(template: &str) -> Result<Vec<String>, BashError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = template.chars();
    while let Some(character) = chars.next() {
        match (quote, character) {
            (None, '\'') => {
                quote = Some('\'');
                started = true;
            }
            (None, '"') => {
                quote = Some('"');
                started = true;
            }
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), value) => word.push(value),
            (_, '\\') => {
                let escaped = chars
                    .next()
                    .ok_or_else(|| error("trailing backslash in command"))?;
                // As in Bash, a backslash inside double quotes escapes only
                // `"`, `\`, `$`, and `` ` ``; elsewhere it stays literal.
                if quote == Some('"') && !matches!(escaped, '"' | '\\' | '$' | '`') {
                    word.push('\\');
                }
                word.push(escaped);
                started = true;
            }
            (None, value) if value.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (_, value) => {
                word.push(value);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(error("unterminated quote in command"));
    }
    if started {
        words.push(word);
    }
    Ok(words)
}
