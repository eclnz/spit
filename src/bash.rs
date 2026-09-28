//! Bash script generation for resolved DAGs: quoting each command argument,
//! and rooting artifact paths at `$SPIT_ROOT`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

use crate::command::{slot, validate_commands, CommandTemplate, Slot};
use crate::model::{
    ArtifactKey, Cardinality, CommandRole, Job, OperationDef, Pipeline, ResolvedDag,
};
use crate::paths::{bound_paths, inspect_paths, output_keys};
use crate::span::Located;
use crate::template::Part;

/// An error that stops a script being generated. Command and path errors
/// are the same type, so they pass through unchanged.
pub type BashError = Located<String>;

fn error(message: impl Into<String>) -> BashError {
    BashError::new(message)
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
    validate_commands(pipeline)?;
    let commands: BTreeMap<_, _> = pipeline
        .commands
        .iter()
        .filter(|command| command.role == CommandRole::Run)
        .map(|command| (command.operation.as_str(), command))
        .collect();
    let mut verifications: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for command in &pipeline.commands {
        if command.role == CommandRole::Verify {
            verifications
                .entry(command.operation.as_str())
                .or_default()
                .push(command);
        }
    }
    let outputs = output_keys(dag);
    let paths = bound_paths(pipeline, dag)?;

    let mut script =
        String::from("#!/usr/bin/env bash\nset -euo pipefail\nSPIT_ROOT=\"${SPIT_ROOT:-.}\"\n\n");
    script.push_str("spit_require() {\n  if [[ ! -e \"$1\" ]]; then\n    printf 'missing artifact: %s\\n' \"$1\" >&2\n    exit 1\n  fi\n}\n\n");
    if !verifications.is_empty() {
        script.push_str("spit_verify() {\n  local job=\"$1\"\n  shift\n  if ! \"$@\"; then\n    printf 'verification failed for job %s: %s\\n' \"$job\" \"$*\" >&2\n    exit 1\n  fi\n}\n\n");
    }

    for (identity, relative) in &paths {
        if !outputs.contains(identity) {
            writeln!(script, "spit_require {}", shell_path(relative)).unwrap();
        }
    }
    if paths.keys().any(|identity| !outputs.contains(identity)) {
        script.push('\n');
    }
    let mut stage = None;
    for job in &dag.jobs {
        if job.stage.as_deref() != stage {
            stage = job.stage.as_deref();
            match stage {
                Some(name) => writeln!(script, "# ===== Stage: {name} =====\n").unwrap(),
                None => writeln!(script, "# ===== Outside stages =====\n").unwrap(),
            }
        }
        let operation = operations.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
        let command = commands.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "no command defined for operation `{}`",
                job.operation
            ))
        })?;
        writeln!(script, "# Job {}: {}", job.id, job.operation).unwrap();
        let parents: BTreeSet<_> = job
            .outputs
            .iter()
            .map(|output| {
                Path::new(&paths[&output.key()])
                    .parent()
                    .and_then(|path| path.to_str())
                    .filter(|path| !path.is_empty())
                    .unwrap_or(".")
                    .to_owned()
            })
            .collect();
        for parent in parents {
            writeln!(script, "mkdir -p -- {}", shell_path(&parent)).unwrap();
        }
        for verification in verifications
            .get(job.operation.as_str())
            .into_iter()
            .flatten()
        {
            let check = render_command(&verification.template, operation, job, &paths)?;
            writeln!(script, "spit_verify {} {check}", job.id).unwrap();
        }
        writeln!(
            script,
            "{}",
            render_command(&command.template, operation, job, &paths)?
        )
        .unwrap();
        for output in &job.outputs {
            writeln!(script, "spit_require {}", shell_path(&paths[&output.key()])).unwrap();
        }
        script.push('\n');
    }
    Ok(script)
}

fn render_command(
    template: &CommandTemplate,
    operation: &OperationDef,
    job: &Job,
    paths: &BTreeMap<ArtifactKey, String>,
) -> Result<String, BashError> {
    let mut args = Vec::new();
    for parts in template.arguments() {
        if let [Part::Placeholder(name)] = parts.as_slice() {
            if let Some(Slot::Input(index)) = slot(operation, name) {
                if operation.inputs[index].cardinality == Cardinality::Many {
                    for artifact in &job.inputs[index] {
                        args.push(shell_path(&paths[&artifact.key()]));
                    }
                    continue;
                }
            }
        }
        let mut arg = String::new();
        for part in parts {
            match part {
                Part::Literal(value) => arg.push_str(&shell_quote(value)),
                Part::Placeholder(name) => {
                    let artifact = match slot(operation, name) {
                        Some(Slot::Output(index)) => job.outputs.get(index),
                        Some(Slot::Input(index)) => {
                            if operation.inputs[index].cardinality == Cardinality::Many {
                                return Err(error(format!(
                                    "many input `{{{name}}}` must be a complete command argument"
                                )));
                            }
                            job.inputs
                                .get(index)
                                .and_then(|artifacts| artifacts.first())
                        }
                        None => {
                            return Err(error(format!(
                                "command for `{}` uses unknown placeholder `{{{name}}}`",
                                operation.name
                            )))
                        }
                    };
                    let artifact = artifact
                        .ok_or_else(|| error(format!("job {} lacks `{{{name}}}`", job.id)))?;
                    arg.push_str(&shell_path(&paths[&artifact.key()]));
                }
            }
        }
        args.push(arg);
    }
    Ok(args.join(" "))
}

fn shell_path(relative: &str) -> String {
    format!("\"$SPIT_ROOT\"/{}", shell_quote(relative))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
