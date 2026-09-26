//! Path binding, source-file validation, and Bash generation for resolved DAGs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::path::Path;

use crate::model::{
    ArtifactInstance, Cardinality, EntityBinding, Job, OperationDef, Pipeline, ProductDef,
    ResolvedDag,
};
use crate::render::render_typed_artifact;

type ArtifactKey = (String, EntityBinding);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BashError {
    /// The pipeline line of the path rule, command, or operation at fault, when known.
    pub line: Option<usize>,
    pub message: String,
}

impl BashError {
    /// Attach a line unless a more specific one is already recorded.
    fn at(mut self, line: Option<usize>) -> Self {
        self.line = self.line.or(line);
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

fn error(message: impl Into<String>) -> BashError {
    BashError {
        line: None,
        message: message.into(),
    }
}

fn command_line(pipeline: &Pipeline, index: usize) -> Option<usize> {
    pipeline.source_lines.command_lines.get(index).copied()
}

/// The pipeline line that declares the path rule used for `product`.
fn path_rule_line(pipeline: &Pipeline, product: &str) -> Option<usize> {
    if pipeline.product_paths.contains_key(product) {
        pipeline.source_lines.paths.get(product).copied()
    } else {
        pipeline.source_lines.default_path
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathRule {
    Explicit(String),
    Default(String),
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverageEntry {
    pub product: String,
    pub source: bool,
    pub rule: PathRule,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathCoverage {
    pub entries: Vec<PathCoverageEntry>,
}

impl PathCoverage {
    /// Missing rules always fail. Strict mode also rejects default fallbacks.
    pub fn validate(&self, strict: bool) -> Result<(), BashError> {
        let missing: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.rule == PathRule::Missing)
            .map(|entry| entry.product.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(error(format!(
                "no path rule for products: {}",
                missing.join(", ")
            )));
        }
        if strict {
            let fallback: Vec<_> = self
                .entries
                .iter()
                .filter(|entry| matches!(entry.rule, PathRule::Default(_)))
                .map(|entry| entry.product.as_str())
                .collect();
            if !fallback.is_empty() {
                return Err(error(format!(
                    "strict paths requires explicit rules for products: {}",
                    fallback.join(", ")
                )));
            }
        }
        Ok(())
    }
}

impl fmt::Display for PathCoverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Product path coverage:")?;
        for entry in &self.entries {
            let role = if entry.source { "source" } else { "output" };
            match &entry.rule {
                PathRule::Explicit(template) => {
                    writeln!(f, "  {} ({role}): explicit {template}", entry.product)?;
                }
                PathRule::Default(template) => {
                    writeln!(f, "  {} ({role}): default {template}", entry.product)?;
                }
                PathRule::Missing => {
                    writeln!(f, "  {} ({role}): MISSING", entry.product)?;
                }
            }
        }
        Ok(())
    }
}

/// Inspect every declared product, including families with no resolved jobs.
pub fn inspect_paths(pipeline: &Pipeline) -> Result<PathCoverage, BashError> {
    let (coverage, errors) = collect_paths(pipeline, &BTreeSet::new());
    match errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(coverage),
    }
}

/// Inspect every path rule, collecting each error. Rules for products in
/// `skip` belong to declarations that already failed and are not checked.
pub(crate) fn collect_paths(
    pipeline: &Pipeline,
    skip: &BTreeSet<String>,
) -> (PathCoverage, Vec<BashError>) {
    let mut errors = Vec::new();
    for name in pipeline.product_paths.keys() {
        if !skip.contains(name)
            && !pipeline
                .products
                .iter()
                .any(|product| &product.name == name)
        {
            errors.push(
                error(format!("path refers to unknown product `{name}`")).at(pipeline
                    .source_lines
                    .paths
                    .get(name)
                    .copied()),
            );
        }
    }
    let outputs: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .map(|invocation| invocation.output_product.as_str())
        .collect();
    let mut entries = Vec::new();
    let mut samples: BTreeMap<String, &str> = BTreeMap::new();
    for product in &pipeline.products {
        let rule = if let Some(template) = pipeline.product_paths.get(&product.name) {
            PathRule::Explicit(template.clone())
        } else if let Some(template) = &pipeline.path_template {
            PathRule::Default(template.clone())
        } else {
            PathRule::Missing
        };
        if rule != PathRule::Missing && !skip.contains(&product.name) {
            let line = path_rule_line(pipeline, &product.name);
            match validate_path_template(pipeline, product) {
                Err(e) => errors.push(e.at(line)),
                // A repeated product name is reported by the resolver as a duplicate.
                Ok(sample) => {
                    if let Some(other) = samples
                        .insert(sample.clone(), &product.name)
                        .filter(|other| *other != product.name)
                    {
                        errors.push(
                            error(format!(
                                "products `{other}` and `{}` bind to the same path `{sample}` for the same entities; include `{{product}}` or distinguish their path rules",
                                product.name
                            ))
                            .at(line),
                        );
                    }
                }
            }
        }
        entries.push(PathCoverageEntry {
            product: product.name.clone(),
            source: !outputs.contains(product.name.as_str()),
            rule,
        });
    }
    (PathCoverage { entries }, errors)
}

/// Bind a product's path rule to placeholder entities, rejecting rules that
/// cannot tell the product's artifacts apart. Returns the sample path.
fn validate_path_template(pipeline: &Pipeline, product: &ProductDef) -> Result<String, BashError> {
    let template = pipeline
        .product_paths
        .get(&product.name)
        .or(pipeline.path_template.as_ref())
        .ok_or_else(|| error(format!("no path template for product `{}`", product.name)))?;
    let placeholders: BTreeSet<_> = parse_template(template)?
        .into_iter()
        .filter_map(|part| match part {
            Part::Placeholder(name) => Some(name),
            Part::Literal(_) => None,
        })
        .collect();
    if !placeholders.contains("entities") {
        if let Some(dimension) = product
            .dimensions
            .iter()
            .find(|dimension| !placeholders.contains(*dimension))
        {
            return Err(error(format!(
                "path template for `{}` omits dimension `{dimension}`; artifacts differing only in `{dimension}` would share a path",
                product.name
            )));
        }
    }
    // Each dimension gets a distinct sample value so that templates naming
    // different dimensions are not mistaken for colliding ones.
    let entities = EntityBinding(
        product
            .dimensions
            .iter()
            .map(|dimension| (dimension.clone(), dimension.clone()))
            .collect(),
    );
    let artifact = ArtifactInstance::new(&product.name, product.artifact_type.clone(), entities);
    let dag = ResolvedDag {
        jobs: Vec::new(),
        product_dimensions: [(product.name.clone(), product.dimensions.clone())]
            .into_iter()
            .collect(),
    };
    bind_path(pipeline, &dag, &artifact)
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
        .enumerate()
        .map(|(index, command)| (command.operation.as_str(), (index, command)))
        .collect();
    let outputs: BTreeSet<_> = dag.jobs.iter().map(|job| key(&job.output)).collect();
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
        let (command_index, command) = commands.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "no command defined for operation `{}`",
                job.operation
            ))
            .at(pipeline
                .source_lines
                .operations
                .get(&operation.name)
                .copied())
        })?;
        let output_path = paths.get(&key(&job.output)).unwrap();
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
            render_command(&command.template, operation, job, &paths)
                .map_err(|e| e.at(command_line(pipeline, *command_index)))?
        )
        .unwrap();
        writeln!(script, "spit_require {}\n", shell_path(output_path)).unwrap();
    }
    Ok(script)
}

/// Check every declared command against its operation without resolving jobs:
/// the template must parse, name only known placeholders, and write `{output}`.
pub fn validate_commands(pipeline: &Pipeline) -> Result<(), BashError> {
    match collect_commands(pipeline, &BTreeSet::new())
        .into_iter()
        .next()
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Check every command, collecting each error. Commands for operations in
/// `skip` belong to declarations that already failed and are not checked.
pub(crate) fn collect_commands(pipeline: &Pipeline, skip: &BTreeSet<String>) -> Vec<BashError> {
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
        let line = command_line(pipeline, index);
        let Some(operation) = operations.get(command.operation.as_str()) else {
            errors.push(
                error(format!(
                    "command refers to unknown operation `{}`",
                    command.operation
                ))
                .at(line),
            );
            continue;
        };
        if operation.inputs.iter().any(|port| port.name == "output") {
            errors.push(
                error(format!(
                    "operation `{}` has an input port named `output`, which shadows `{{output}}`",
                    operation.name
                ))
                .at(line),
            );
        } else if !seen.insert(command.operation.as_str()) {
            errors.push(
                error(format!(
                    "duplicate command for operation `{}`",
                    command.operation
                ))
                .at(line),
            );
        } else if let Err(e) = check_command_placeholders(&command.template, operation) {
            errors.push(e.at(line));
        }
    }
    errors
}

/// Check placeholder brackets in a path template.
pub(crate) fn check_path_template_syntax(template: &str) -> Result<(), BashError> {
    parse_template(template).map(|_| ())
}

/// Check quoting and placeholder brackets in a command template.
pub(crate) fn check_command_syntax(template: &str) -> Result<(), BashError> {
    let words = split_words(template)?;
    if words.is_empty() {
        return Err(error("command template must not be empty"));
    }
    for word in words {
        parse_template(&word)?;
    }
    Ok(())
}

fn check_command_placeholders(template: &str, operation: &OperationDef) -> Result<(), BashError> {
    check_command_syntax(template)?;
    let single_many =
        operation.inputs.len() == 1 && operation.inputs[0].cardinality == Cardinality::Many;
    let mut uses_output = false;
    for word in split_words(template)? {
        let parts = parse_template(&word)?;
        let whole = parts.len() == 1;
        for part in parts {
            let Part::Placeholder(name) = part else {
                continue;
            };
            if name == "output" {
                uses_output = true;
                continue;
            }
            let port = operation
                .inputs
                .iter()
                .find(|port| port.name == name)
                .or_else(|| (single_many && name == "inputs").then(|| &operation.inputs[0]))
                .ok_or_else(|| {
                    error(format!(
                        "command for `{}` uses unknown placeholder `{{{name}}}`",
                        operation.name
                    ))
                })?;
            if port.cardinality == Cardinality::Many && !whole {
                return Err(error(format!(
                    "many input `{{{name}}}` must be a complete command argument"
                )));
            }
        }
    }
    if !uses_output {
        return Err(error(format!(
            "command for `{}` must use `{{output}}`",
            operation.name
        )));
    }
    Ok(())
}

/// Validate concrete artifact path bindings without requiring commands.
pub fn validate_concrete_paths(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<(), BashError> {
    bound_paths(pipeline, dag)?;
    Ok(())
}

/// Check the files needed to start the resolved DAG under a dataset root.
/// Derived outputs are deliberately excluded because the pipeline creates them.
pub fn validate_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<usize, BashError> {
    if !root.is_dir() {
        return Err(error(format!(
            "source root is not a directory: `{}`",
            root.display()
        )));
    }
    inspect_paths(pipeline)?.validate(false)?;
    let paths = bound_paths(pipeline, dag)?;
    let outputs: BTreeSet<_> = dag.jobs.iter().map(|job| key(&job.output)).collect();
    let mut checked = 0;
    for (artifact, relative) in paths {
        if outputs.contains(&artifact) {
            continue;
        }
        let full_path = root.join(&relative);
        if !full_path.is_file() {
            return Err(error(format!(
                "missing source file for `{}[{}]`: `{}`",
                artifact.0,
                artifact.1,
                full_path.display()
            )));
        }
        checked += 1;
    }
    Ok(checked)
}

/// Inspect the resolved jobs and bound paths before expanding any commands.
pub fn render_bound_dag(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<String, BashError> {
    inspect_paths(pipeline)?.validate(false)?;
    let paths = bound_paths(pipeline, dag)?;
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let mut output = String::new();
    for (index, job) in dag.jobs.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let operation = operations.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
        writeln!(output, "Job {}", job.id).unwrap();
        writeln!(output, "  operation: {}", job.operation).unwrap();
        writeln!(output, "  inputs:").unwrap();
        for (input_index, input) in job.inputs.iter().enumerate() {
            let port = if operation.inputs.len() == 1
                && operation.inputs[0].cardinality == Cardinality::Many
            {
                &operation.inputs[0]
            } else {
                operation.inputs.get(input_index).ok_or_else(|| {
                    error(format!(
                        "job {} has more inputs than operation ports",
                        job.id
                    ))
                })?
            };
            writeln!(
                output,
                "    {}: {}",
                port.name,
                render_typed_artifact(dag, input)
            )
            .unwrap();
            writeln!(output, "      path: {}", paths[&key(input)]).unwrap();
        }
        writeln!(output, "  output:").unwrap();
        writeln!(output, "    {}", render_typed_artifact(dag, &job.output)).unwrap();
        writeln!(output, "      path: {}", paths[&key(&job.output)]).unwrap();
        if !job.dependencies.is_empty() {
            let dependencies: Vec<_> = job.dependencies.iter().map(ToString::to_string).collect();
            writeln!(output, "  depends_on: {}", dependencies.join(", ")).unwrap();
        }
    }
    Ok(output)
}

fn bound_paths(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
) -> Result<BTreeMap<ArtifactKey, String>, BashError> {
    let mut paths = BTreeMap::new();
    let mut owners = BTreeMap::new();
    for artifact in dag
        .jobs
        .iter()
        .flat_map(|job| job.inputs.iter().chain(std::iter::once(&job.output)))
    {
        let identity = key(artifact);
        if paths.contains_key(&identity) {
            continue;
        }
        let line = path_rule_line(pipeline, &artifact.product);
        let relative = bind_path(pipeline, dag, artifact).map_err(|e| e.at(line))?;
        if let Some(previous) = owners.insert(relative.clone(), identity.clone()) {
            return Err(error(format!(
                "artifacts `{}[{}]` and `{}[{}]` bind to the same path `{relative}`",
                previous.0, previous.1, identity.0, identity.1
            ))
            .at(line));
        }
        paths.insert(identity, relative);
    }
    Ok(paths)
}

fn key(artifact: &ArtifactInstance) -> ArtifactKey {
    (artifact.product.clone(), artifact.entities.clone())
}

fn bind_path(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    artifact: &ArtifactInstance,
) -> Result<String, BashError> {
    let template = pipeline
        .product_paths
        .get(&artifact.product)
        .or(pipeline.path_template.as_ref())
        .ok_or_else(|| {
            error(format!(
                "no path template for product `{}`",
                artifact.product
            ))
        })?;
    let mut relative = String::new();
    for part in parse_template(template)? {
        match part {
            Part::Literal(value) => relative.push_str(&value),
            Part::Placeholder(name) if name == "product" => {
                relative.push_str(&artifact.product);
            }
            Part::Placeholder(name) if name == "entities" => {
                let dimensions = dag
                    .product_dimensions
                    .get(&artifact.product)
                    .ok_or_else(|| error(format!("unknown product `{}`", artifact.product)))?;
                let bindings = dimensions
                    .iter()
                    .map(|dimension| {
                        let value = artifact.entities.0.get(dimension).ok_or_else(|| {
                            error(format!(
                                "artifact `{artifact}` lacks dimension `{dimension}`"
                            ))
                        })?;
                        Ok(format!(
                            "{}={}",
                            encode_component(dimension),
                            encode_component(value)
                        ))
                    })
                    .collect::<Result<Vec<_>, BashError>>()?;
                if bindings.is_empty() {
                    relative.push_str("global");
                } else {
                    relative.push_str(&bindings.join("__"));
                }
            }
            Part::Placeholder(dimension) => {
                let value = artifact.entities.0.get(&dimension).ok_or_else(|| {
                    error(format!(
                        "path template for `{}` uses absent dimension `{dimension}`",
                        artifact.product
                    ))
                })?;
                relative.push_str(&encode_component(value));
            }
        }
    }
    if relative
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(error(format!(
            "path for `{artifact}` must be a relative path without `.` or `..`: `{relative}`"
        )));
    }
    Ok(relative)
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
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
        let parts = parse_template(&word)?;
        if let [Part::Placeholder(name)] = parts.as_slice() {
            if let Some(artifacts) = many_input(operation, job, name) {
                for artifact in artifacts {
                    args.push(shell_path(paths.get(&key(artifact)).unwrap()));
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
                    arg.push_str(&shell_path(paths.get(&key(&job.output)).unwrap()));
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
                    arg.push_str(&shell_path(paths.get(&key(artifact)).unwrap()));
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum Part {
    Literal(String),
    Placeholder(String),
}

fn parse_template(template: &str) -> Result<Vec<Part>, BashError> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '{' => {
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') if name.is_empty() => {
                            return Err(error(format!("empty placeholder `{{}}` in `{template}`")))
                        }
                        Some('}') => break,
                        Some('{') => {
                            return Err(error(format!(
                                "nested `{{` in placeholder in `{template}`"
                            )))
                        }
                        Some(value) => name.push(value),
                        None => return Err(error(format!("unclosed `{{` in `{template}`"))),
                    }
                }
                parts.push(Part::Placeholder(name));
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '}' => return Err(error(format!("unmatched `}}` in `{template}`"))),
            value => literal.push(value),
        }
    }
    if !literal.is_empty() || parts.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(parts)
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
                word.push(
                    chars
                        .next()
                        .ok_or_else(|| error("trailing backslash in command"))?,
                );
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
