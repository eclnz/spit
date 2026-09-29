//! Step 4, the Bash backend: turn a bound DAG into a script, quoting each
//! argument and rooting every path at `$SPIT_ROOT`. It reads only the
//! `.spitdag`: no pipeline, path rule or command template.

use std::collections::BTreeSet;
use std::fmt::{self, Write};
use std::path::Path;

use crate::spitdag::{ArgPart, Argument, BoundDag};

/// Why a script cannot be written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BashError(String);

impl BashError {
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BashError {}

/// A script that runs every job of `dag` in order.
pub fn render_bash(dag: &BoundDag) -> Result<String, BashError> {
    let verifies = dag.jobs.iter().any(|job| !job.verify.is_empty());
    let mut script =
        String::from("#!/usr/bin/env bash\nset -euo pipefail\nSPIT_ROOT=\"${SPIT_ROOT:-.}\"\n");
    // A root such as `-data` would make every path read as an option.
    script.push_str("case $SPIT_ROOT in -*) SPIT_ROOT=\"./$SPIT_ROOT\" ;; esac\n\n");
    script.push_str("spit_require() {\n  if [[ ! -e \"$1\" ]]; then\n    printf 'missing artifact: %s\\n' \"$1\" >&2\n    exit 1\n  fi\n}\n\n");
    if verifies {
        script.push_str("spit_verify() {\n  local job=\"$1\"\n  shift\n  if ! \"$@\"; then\n    printf 'verification failed for job %s: %s\\n' \"$job\" \"$*\" >&2\n    exit 1\n  fi\n}\n\n");
    }

    let external = dag.external_inputs();
    for artifact in &external {
        writeln!(script, "spit_require {}", shell_path(&artifact.path)).unwrap();
    }
    if !external.is_empty() {
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
        let command = job.command.as_ref().ok_or_else(|| {
            BashError(format!(
                "no command defined for operation `{}`",
                job.operation
            ))
        })?;
        writeln!(script, "# Job {}: {}", job.id, job.operation).unwrap();
        let parents: BTreeSet<_> = job
            .outputs
            .iter()
            .map(|(_, output)| {
                Path::new(&output.path)
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
        for check in &job.verify {
            writeln!(script, "spit_verify {} {}", job.id, shell_command(check)).unwrap();
        }
        writeln!(script, "{}", shell_command(command)).unwrap();
        for (_, output) in &job.outputs {
            writeln!(script, "spit_require {}", shell_path(&output.path)).unwrap();
        }
        script.push('\n');
    }
    Ok(script)
}

fn shell_command(arguments: &[Argument]) -> String {
    let arguments: Vec<_> = arguments
        .iter()
        .map(|argument| {
            argument
                .iter()
                .map(|part| match part {
                    ArgPart::Text(text) => shell_quote(text),
                    ArgPart::Path(path) => shell_path(path),
                })
                .collect::<String>()
        })
        .collect();
    arguments.join(" ")
}

fn shell_path(relative: &str) -> String {
    format!("\"$SPIT_ROOT\"/{}", shell_quote(relative))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
