//! Bash script generation for resolved DAGs, and static checks of commands.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::ops::Range;
use std::path::Path;

use crate::model::{
    ArtifactKey, Cardinality, CommandRole, DefaultPort, Job, OperationDef, Pipeline, ResolvedDag,
};
use crate::parser::SourceMap;
use crate::paths::{bound_paths, inspect_paths, output_keys, PathError};
use crate::span::Place;
use crate::template::{parse_template, Part};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BashError {
    /// The pipeline line of the path rule, command, or operation at fault, when known.
    pub line: Option<usize>,
    /// The byte range in that line, when known.
    pub columns: Option<Range<usize>>,
    pub message: String,
    /// Text within the command or path template that the error is about,
    /// such as one `{placeholder}`.
    pub(crate) focus: Option<String>,
}

impl BashError {
    /// Attach a place unless a more specific one is already recorded.
    fn at(mut self, place: Option<Place>) -> Self {
        if self.line.is_none() {
            if let Some(place) = place {
                self.line = Some(place.line);
                self.columns = Some(place.columns);
            }
        }
        self
    }

    fn focus(mut self, text: impl Into<String>) -> Self {
        self.focus = Some(text.into());
        self
    }
}

impl fmt::Display for BashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for BashError {}

impl From<PathError> for BashError {
    fn from(error: PathError) -> Self {
        Self {
            line: error.line,
            columns: error.columns,
            message: error.message,
            focus: error.focus,
        }
    }
}

fn error(message: impl Into<String>) -> BashError {
    BashError {
        line: None,
        columns: None,
        message: message.into(),
        focus: None,
    }
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

/// Check every declared command against its operation without resolving jobs:
/// the template must parse, name only known placeholders, and write every
/// output; a `verify` command may read inputs only.
pub fn validate_commands(pipeline: &Pipeline) -> Result<(), BashError> {
    let lines = SourceMap::default();
    match collect_commands(pipeline, &lines, &BTreeSet::new())
        .into_iter()
        .next()
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Check every command, collecting each error. Commands for operations in
/// `skip` belong to declarations that already failed and are not checked.
pub(crate) fn collect_commands(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<BashError> {
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, command) in pipeline.commands.iter().enumerate() {
        if skip.contains(&command.operation) {
            continue;
        }
        let line = lines.command(index);
        let Some(operation) = operations.get(command.operation.as_str()) else {
            errors.push(
                error(format!(
                    "command refers to unknown operation `{}`",
                    command.operation
                ))
                .at(line)
                .focus(&command.operation),
            );
            continue;
        };
        if let Some(port) = operation.inputs.iter().find(|port| {
            operation
                .outputs
                .iter()
                .any(|output| output.name == port.name)
        }) {
            errors.push(
                error(format!(
                    "operation `{}` has an input port named `{}`, which shadows `{{{}}}`",
                    operation.name, port.name, port.name
                ))
                .at(line.clone())
                .focus(&command.operation),
            );
        } else if command.role == CommandRole::Run && !seen.insert(command.operation.as_str()) {
            errors.push(
                error(format!(
                    "duplicate command for operation `{}`",
                    command.operation
                ))
                .at(line)
                .focus(&command.operation),
            );
        } else if let Err(e) =
            check_command_placeholders(&command.template, operation, command.role)
        {
            errors.push(e.at(line));
        }
    }
    errors
}

/// A command's template, split into words and parsed once when the
/// command is read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandTemplate {
    text: String,
    /// Each word as Bash splits and unquotes it, then its literal text and
    /// `{placeholders}`.
    words: Vec<Vec<Part>>,
}

impl CommandTemplate {
    /// Split and parse a template such as `sort -o {output} {input}`,
    /// checking its quotes and braces.
    pub fn parse(text: impl Into<String>) -> Result<Self, BashError> {
        let text = text.into();
        let words = split_words(&text)?
            .iter()
            .map(|word| parse_template(word).map_err(error))
            .collect::<Result<Vec<_>, _>>()?;
        if words.is_empty() {
            return Err(error("command template must not be empty"));
        }
        Ok(Self { text, words })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    fn words(&self) -> &[Vec<Part>] {
        &self.words
    }
}

impl fmt::Display for CommandTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq<str> for CommandTemplate {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<&str> for CommandTemplate {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

/// What a placeholder in a command refers to.
enum Slot {
    Input(usize),
    Output(usize),
}

fn slot(operation: &OperationDef, name: &str) -> Option<Slot> {
    if let Some(index) = operation.inputs.iter().position(|port| port.name == name) {
        return Some(Slot::Input(index));
    }
    if let Some(index) = operation.outputs.iter().position(|port| port.name == name) {
        return Some(Slot::Output(index));
    }
    match operation.inputs.as_slice() {
        [port] if port.cardinality == Cardinality::Many && name == DefaultPort::Inputs.name() => {
            Some(Slot::Input(0))
        }
        _ => None,
    }
}

fn check_command_placeholders(
    template: &CommandTemplate,
    operation: &OperationDef,
    role: CommandRole,
) -> Result<(), BashError> {
    let mut written = BTreeSet::new();
    for parts in template.words() {
        let whole = parts.len() == 1;
        for part in parts {
            let Part::Placeholder(name) = part else {
                continue;
            };
            let unknown = || {
                error(format!(
                    "command for `{}` uses unknown placeholder `{{{name}}}`",
                    operation.name
                ))
                .focus(format!("{{{name}}}"))
            };
            match slot(operation, name).ok_or_else(unknown)? {
                Slot::Output(index) => {
                    if role == CommandRole::Verify {
                        return Err(error(format!(
                            "verify for `{}` cannot use output `{{{name}}}`, which does not exist until the command runs",
                            operation.name
                        ))
                        .focus(format!("{{{name}}}")));
                    }
                    written.insert(index);
                }
                Slot::Input(index) => {
                    if operation.inputs[index].cardinality == Cardinality::Many && !whole {
                        return Err(error(format!(
                            "many input `{{{name}}}` must be a complete command argument"
                        ))
                        .focus(format!("{{{name}}}")));
                    }
                }
            }
        }
    }
    if role == CommandRole::Run {
        if let Some(port) = operation
            .outputs
            .iter()
            .enumerate()
            .find(|(index, _)| !written.contains(index))
            .map(|(_, port)| port)
        {
            return Err(error(format!(
                "command for `{}` must use `{{{}}}`",
                operation.name, port.name
            )));
        }
    }
    Ok(())
}

fn render_command(
    template: &CommandTemplate,
    operation: &OperationDef,
    job: &Job,
    paths: &BTreeMap<ArtifactKey, String>,
) -> Result<String, BashError> {
    let mut args = Vec::new();
    for parts in template.words() {
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
