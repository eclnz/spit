mod screen_records;

use std::fmt;

use rustc_hash::FxHashMap;

use crate::check::{step_checks, CheckIndex, StepCheck, When};
use crate::command::{facet, slot, Facet, Slot};
use crate::compile::validate_commands;
use crate::model::{
    ArtifactId, CallId, Cardinality, CommandDef, CommandRole, DagStep, Job, OperationDef, Pipeline,
    PipelineIndex, ResolvedDag, Resources,
};
use crate::paths::{bound_paths, check_rules, BoundPaths};
use crate::spitdag::{
    ArgPart, Argument, BoundCall, BoundCheck, BoundCollection, BoundCommand, BoundDag, BoundJob,
    BoundStep, StepCall, StepCheck as BoundStepCheck,
};
use crate::template::builtin::CHECKED_PATH;
use crate::template::PathError;
use crate::template::{CollectionRead, CommandPart};
use crate::template::{CommandError, CommandTemplate, Stream};

pub fn bind_dag(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<BoundDag, BindError> {
    check_rules(pipeline, dag)?;
    validate_commands(pipeline)?;
    let paths = bound_paths(pipeline, dag)?;
    bind_jobs(pipeline, dag, paths)
}

pub fn bind_dag_with(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    paths: BoundPaths,
) -> Result<BoundDag, BindError> {
    validate_commands(pipeline)?;
    bind_jobs(pipeline, dag, paths)
}

fn bind_jobs(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    bound: BoundPaths,
) -> Result<BoundDag, BindError> {
    let BoundPaths { paths, under } = bound;
    let index = PipelineIndex::new(pipeline);
    let checks = CheckIndex::new(&index);
    let mut commands: FxHashMap<&str, Vec<&CommandDef>> = FxHashMap::default();
    for command in &pipeline.commands {
        commands
            .entry(command.operation.as_str())
            .or_default()
            .push(command);
    }
    let resources: FxHashMap<&str, Resources> = pipeline
        .resources
        .iter()
        .map(|line| (line.operation.as_str(), line.resources))
        .collect();
    let declared_in = pipeline.operation_files();
    let steps = dag
        .steps
        .iter()
        .map(|step| {
            let needs = resources
                .get(step.operation.as_str())
                .copied()
                .unwrap_or_default();
            let here = here_of(pipeline, &declared_in, &step.operation);
            StepCommands::new(&index, &checks, &commands, step, needs, here)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut produced = vec![false; dag.artifacts.len()];
    for &output in dag
        .jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .chain(dag.views.keys())
    {
        produced[output.index()] = true;
    }
    let folders: Vec<bool> = dag
        .artifacts
        .products()
        .map(|(product, _)| index.is_folder(product))
        .collect();
    let jobs = dag
        .jobs
        .iter()
        .map(|job| {
            let step = &steps[job.step.index()];
            bind_job(step, dag, &paths, &produced, &folders, job)
        })
        .collect::<Result<_, _>>()?;
    let dimensions = dag
        .artifacts
        .products()
        .map(|(product, _)| {
            dag.product_dimensions
                .get(product)
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let paths = paths.into_iter().map(Option::unwrap_or_default).collect();
    let mut bound = BoundDag::new(
        dag.artifacts.clone(),
        paths,
        under,
        dimensions,
        folders,
        steps.iter().map(StepCommands::bound).collect(),
        jobs,
    );
    bound.views.clone_from(&dag.views);
    screen_records::bind_screen_records(&mut bound, &index)?;
    bound.pipeline_files.clone_from(&pipeline.files);
    bound.pipeline_folder.clone_from(&pipeline.folder);
    let files: Vec<Option<usize>> = pipeline
        .calls
        .iter()
        .map(|call| declared_in.get(call.operation.as_str()).copied().flatten())
        .collect();
    for step in &mut bound.steps {
        if let Some(origin) = &mut step.origin {
            origin.file = match origin.call {
                Some(call) => files[call],
                None => (!pipeline.files.is_empty()).then_some(0),
            };
        }
    }
    bound.calls = pipeline
        .calls
        .iter()
        .zip(&files)
        .map(|(call, &file)| BoundCall {
            operation: call.operation.clone(),
            instance: call.outputs[0].clone(),
            parent: call.parent.map(CallId::index),
            file,
            at_file: match call.parent {
                Some(parent) => files[parent.index()],
                None => (!pipeline.files.is_empty()).then_some(0),
            },
            at_line: call.place.line,
        })
        .collect();
    Ok(bound)
}

fn here_of(
    pipeline: &Pipeline,
    declared_in: &FxHashMap<&str, Option<usize>>,
    operation: &str,
) -> Option<ArgPart> {
    let folder = pipeline.folder.as_deref()?;
    let file = declared_in.get(operation).copied().flatten()?;
    let relative = pipeline.files[file]
        .path
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let text = if relative.is_empty() {
        folder.to_owned()
    } else {
        format!("{folder}/{relative}")
    };
    Some(ArgPart::Here {
        relative: relative.to_owned(),
        text,
    })
}

struct StepCommands<'p> {
    step: &'p DagStep,
    operation: &'p OperationDef,
    run: Option<&'p CommandDef>,
    verify: Vec<&'p CommandDef>,
    checks: Vec<StepCheck<'p>>,
    resources: Resources,
    collection: Option<BoundCollection>,
    cores: String,
    here: Option<ArgPart>,
}

impl<'p> StepCommands<'p> {
    fn new(
        index: &PipelineIndex<'p>,
        checks: &CheckIndex<'p>,
        commands: &FxHashMap<&str, Vec<&'p CommandDef>>,
        step: &'p DagStep,
        resources: Resources,
        here: Option<ArgPart>,
    ) -> Result<Self, BindError> {
        let operation = index.operation(&step.operation).ok_or_else(|| {
            BindError::Dag(format!(
                "unknown operation `{}` in resolved DAG",
                step.operation
            ))
        })?;
        let commands = |role: CommandRole| {
            commands
                .get(step.operation.as_str())
                .into_iter()
                .flatten()
                .copied()
                .filter(move |command| command.role == role)
        };
        let invocation = step
            .outputs
            .first()
            .and_then(|output| index.producer(output))
            .map(|(invocation, _)| invocation);
        let collection = step
            .collection
            .as_ref()
            .filter(|_| invocation.is_some())
            .map(|rule| BoundCollection {
                ports: rule.ports.clone(),
                varied: rule.member.clone(),
                minimum: rule.minimum,
            });
        Ok(Self {
            step,
            collection,
            operation,
            run: commands(CommandRole::Run).next(),
            verify: commands(CommandRole::Verify).collect(),
            checks: step_checks(index, checks, operation, invocation),
            resources,
            cores: resources.cores.to_string(),
            here,
        })
    }

    fn bound(&self) -> BoundStep {
        BoundStep {
            operation: self.step.operation.clone(),
            stage: self.step.stage.clone(),
            collection: self.collection.clone(),
            inputs: self
                .operation
                .inputs
                .iter()
                .map(|port| port.name.clone())
                .collect(),
            outputs: self
                .operation
                .outputs
                .iter()
                .map(|port| port.name.clone())
                .collect(),
            seeds: self
                .operation
                .outputs
                .iter()
                .enumerate()
                .filter_map(|(output, port)| {
                    let seed = port.seed.as_ref()?;
                    let input = self
                        .operation
                        .inputs
                        .iter()
                        .position(|input| input.name == *seed)?;
                    Some((output, input))
                })
                .collect(),
            origin: self.step.line.map(|line| StepCall {
                call: self.step.origin.as_ref().map(|origin| origin.call.index()),
                file: None,
                line,
            }),
            checks: self
                .checks
                .iter()
                .map(|check| BoundStepCheck {
                    when: check.when,
                    port: check.port,
                    written: check.written.clone(),
                    kind: check.check.kind,
                    product_scope: check.product_scope,
                })
                .collect(),
            resources: self.resources,
            tools: self.operation.tools.clone(),
            anchored: self.operation.anchored,
        }
    }
}

fn bind_job(
    step: &StepCommands<'_>,
    dag: &ResolvedDag,
    paths: &[Option<String>],
    produced: &[bool],
    folders: &[bool],
    job: &Job,
) -> Result<BoundJob, BindError> {
    let operation = step.operation;
    if job.inputs.len() != operation.inputs.len() || job.outputs.len() != operation.outputs.len() {
        return Err(BindError::Dag(format!(
            "job {} has different ports from operation `{}`",
            job.id, operation.name
        )));
    }
    let bound = |id: ArtifactId| match &paths[id.index()] {
        Some(_) => Ok(id),
        None => Err(BindError::Dag(format!(
            "no path is bound for `{}`",
            dag.artifact(id)
        ))),
    };
    let inputs = job
        .inputs
        .iter()
        .map(|artifacts| artifacts.iter().map(|&artifact| bound(artifact)).collect())
        .collect::<Result<_, _>>()?;
    let outputs = job
        .outputs
        .iter()
        .map(|&output| bound(output))
        .collect::<Result<_, _>>()?;
    let redirected = |template: &CommandTemplate, stream: Stream| {
        let Some(name) = template.redirected(stream) else {
            return Ok(None);
        };
        let artifact = match slot(operation, name) {
            Some(Slot::Input(index)) => job.inputs.get(index).and_then(|a| a.first()),
            Some(Slot::Output(index)) => job.outputs.get(index),
            Some(Slot::Cores | Slot::Here | Slot::Dimension) | None => None,
        };
        let artifact = *artifact
            .ok_or_else(|| CommandError::new(format!("job {} lacks `{{{name}}}`", job.id)))?;
        if stream == Stream::Stdin && folders[dag.artifacts.product_of(artifact) as usize] {
            return Err(BindError::Command(CommandError::new(format!(
                "`< {{{name}}}` in the command for `{}` reads `{}` on its stdin, but it is a folder, and stdin reads one file",
                operation.name,
                dag.artifact(artifact)
            ))));
        }
        Ok(Some(artifact))
    };
    let commands = step
        .run
        .filter(|_| job.missing.iter().all(|input| input.member.is_some()))
        .map_or(&[][..], |run| run.lines.as_slice())
        .iter()
        .map(|template| {
            if let Some(expression) = template.expression() {
                for name in expression.inputs() {
                    let artifact = match slot(operation, name) {
                        Some(Slot::Input(index)) => job.inputs.get(index).and_then(|a| a.first()),
                        _ => None,
                    }
                    .ok_or_else(|| {
                        CommandError::new(format!(
                            "job {} lacks calculation input `{name}`",
                            job.id
                        ))
                    })?;
                    if folders[dag.artifacts.product_of(*artifact) as usize] {
                        return Err(BindError::Command(CommandError::new(format!(
                            "calculation for `{}` reads `{name}`, but `{}` is a folder",
                            operation.name,
                            dag.artifact(*artifact)
                        ))));
                    }
                }
                return Ok(BoundCommand {
                    arguments: vec![vec![ArgPart::Calculate(expression.clone())]],
                    stdin: None,
                    stdout: job.outputs.first().copied(),
                });
            }
            Ok(BoundCommand {
                arguments: expand(template.arguments(), step, dag, folders, paths, job)?,
                stdin: redirected(template, Stream::Stdin)?,
                stdout: redirected(template, Stream::Stdout)?,
            })
        })
        .collect::<Result<_, BindError>>()?;
    Ok(BoundJob {
        id: job.id,
        step: job.step,
        inputs,
        outputs,
        depends_on: job.dependencies.clone(),
        commands,
        missing: job.missing.clone(),
        verify: step
            .verify
            .iter()
            .filter(|_| job.missing.iter().all(|input| input.member.is_some()))
            .flat_map(|command| &command.lines)
            .map(|template| expand(template.arguments(), step, dag, folders, paths, job))
            .collect::<Result<_, _>>()?,
        checks: bind_checks(&step.checks, produced, job),
    })
}

fn collection(
    read: &CollectionRead,
    operation: &OperationDef,
    dag: &ResolvedDag,
    paths: &[Option<String>],
    job: &Job,
) -> Result<ArgPart, BindError> {
    let (port, field) = facet(&read.subject);
    let Some(Slot::Input(index)) = slot(operation, port) else {
        unreachable!("`validate_commands` gives `@join` and `@list` a many input")
    };
    let artifacts = job
        .inputs
        .get(index)
        .ok_or_else(|| CommandError::new(format!("job {} lacks `{port}`", job.id)))?;
    let separator = read.separator.as_deref();
    let breaks = |member: &str| match separator {
        Some(separator) => !separator.is_empty() && member.contains(separator),
        None => member.contains(['\n', '\r']),
    };
    let refuse = |artifact: ArtifactId, what: &str, member: &str| {
        let why = match separator {
            Some(separator) => format!("holds the separator `{}`", separator.escape_debug()),
            None => "holds a line break".to_owned(),
        };
        BindError::Command(CommandError::new(format!(
            "`{}` in the command for `{}` cannot be split back into its members: the {what} of `{}`, `{}`, {why}",
            read,
            operation.name,
            dag.artifact(artifact),
            member.escape_debug()
        )))
    };
    if let Facet::Dimension(dimension) = field {
        let separator = separator.expect("only `@join` reads a dimension's values");
        let mut values = Vec::with_capacity(artifacts.len());
        for &artifact in artifacts {
            let value = dag
                .artifacts
                .entities(artifact)
                .get(dimension)
                .ok_or_else(|| {
                    CommandError::new(format!("job {} lacks `{}`", job.id, read.subject))
                })?;
            if breaks(value) {
                return Err(refuse(artifact, "value", value));
            }
            values.push(value.to_owned());
        }
        return Ok(ArgPart::JoinedValues {
            port: index,
            values,
            separator: separator.into(),
        });
    }
    for &artifact in artifacts {
        let path = paths[artifact.index()]
            .as_deref()
            .expect("a bound input has a path");
        if breaks(path) {
            return Err(refuse(artifact, "path", path));
        }
    }
    Ok(ArgPart::Collection {
        port: index,
        artifacts: artifacts.clone(),
        separator: read.separator.clone(),
    })
}

fn bind_checks(checks: &[StepCheck<'_>], produced: &[bool], job: &Job) -> Vec<BoundCheck> {
    let mut bound = Vec::new();
    let mut start = 0;
    while start < checks.len() {
        let (when, port) = (checks[start].when, checks[start].port);
        let end = start
            + checks[start..]
                .iter()
                .take_while(|check| check.when == when && check.port == port)
                .count();
        let artifacts = match when {
            When::Before => job.inputs.get(port).map_or(&[][..], Vec::as_slice),
            When::After => job.outputs.get(port).map_or(&[][..], std::slice::from_ref),
        };
        for &artifact in artifacts {
            for (index, check) in checks.iter().enumerate().take(end).skip(start) {
                if check.covered && produced[artifact.index()] {
                    continue;
                }
                bound.push(BoundCheck {
                    check: index,
                    artifact,
                    command: check_command(check, artifact),
                });
            }
        }
        start = end;
    }
    bound
}

fn check_command(check: &StepCheck<'_>, artifact: ArtifactId) -> Vec<Argument> {
    let definition = check.check;
    definition
        .template
        .arguments()
        .iter()
        .map(|parts| {
            parts
                .iter()
                .map(|part| match part {
                    CommandPart::Literal(text) => ArgPart::Text(text.clone()),
                    CommandPart::Placeholder(name) if name == CHECKED_PATH => {
                        ArgPart::Path(artifact)
                    }
                    CommandPart::Scalar(_) | CommandPart::Collection(_) => {
                        unreachable!(
                            "`check_template` rejects a check that reads a scalar or a collection"
                        )
                    }
                    CommandPart::Placeholder(name) => {
                        let index = definition
                            .parameters
                            .iter()
                            .position(|parameter| parameter == name)
                            .expect("collect_checks makes sure a check reads only its parameters");
                        ArgPart::Text(check.arguments[index].clone())
                    }
                })
                .collect()
        })
        .collect()
}

fn expand(
    template: &[Vec<CommandPart>],
    step: &StepCommands<'_>,
    dag: &ResolvedDag,
    folders: &[bool],
    paths: &[Option<String>],
    job: &Job,
) -> Result<Vec<Argument>, BindError> {
    let operation = step.operation;
    let path = |artifact: &ArtifactId| Ok::<_, BindError>(ArgPart::Path(*artifact));
    let lacks = |name: &str| CommandError::new(format!("job {} lacks `{{{name}}}`", job.id));
    let value = |artifact: ArtifactId, dimension: &str, name: &str| {
        dag.artifacts
            .entities(artifact)
            .get(dimension)
            .map(|value| ArgPart::Text(value.to_owned()))
            .ok_or_else(|| lacks(name))
    };
    let mut arguments = Vec::new();
    for parts in template {
        if let [CommandPart::Placeholder(name)] = parts.as_slice() {
            let (port, facet) = facet(name);
            if let Some(Slot::Input(index)) = slot(operation, port) {
                if operation.inputs[index].cardinality == Cardinality::Many {
                    let artifacts = job.inputs.get(index).ok_or_else(|| lacks(name))?;
                    for (offset, &artifact) in artifacts.iter().enumerate() {
                        let value = match facet {
                            Facet::Dimension(dimension) => value(artifact, dimension, name)?,
                            _ => path(&artifact)?,
                        };
                        arguments.push(vec![ArgPart::Member {
                            port: index,
                            offset,
                            value: Box::new(value),
                        }]);
                    }
                    continue;
                }
            }
        }
        let mut argument = Vec::new();
        for part in parts {
            match part {
                CommandPart::Literal(value) => argument.push(ArgPart::Text(value.clone())),
                CommandPart::Collection(read) => {
                    argument.push(collection(read, operation, dag, paths, job)?);
                }
                CommandPart::Scalar(read) => {
                    let artifact = match slot(operation, &read.port) {
                        Some(Slot::Input(index)) => job.inputs.get(index).and_then(|a| a.first()),
                        _ => None,
                    };
                    let artifact = *artifact.ok_or_else(|| lacks(&read.port))?;
                    if folders[dag.artifacts.product_of(artifact) as usize] {
                        return Err(BindError::Command(CommandError::new(format!(
                            "`{read}` in the command for `{}` reads the number in `{}`, but it is a folder, and `{}` reads one file",
                            operation.name,
                            dag.artifact(artifact),
                            crate::template::SCALAR
                        ))));
                    }
                    argument.push(ArgPart::Scalar {
                        artifact,
                        min: read.min.clone(),
                        max: read.max.clone(),
                    });
                }
                CommandPart::Placeholder(name) => {
                    let (port, facet) = facet(name);
                    let slot = slot(operation, port);
                    let artifact = match slot {
                        Some(Slot::Cores) => {
                            argument.push(ArgPart::Text(step.cores.clone()));
                            continue;
                        }
                        Some(Slot::Here) => {
                            let here = step.here.clone().ok_or_else(|| {
                                CommandError::new(format!(
                                    "`{{@here}}` in the command for `{}` needs a pipeline read from a file",
                                    operation.name
                                ))
                            })?;
                            argument.push(here);
                            continue;
                        }
                        Some(Slot::Dimension) => {
                            let value = job
                                .outputs
                                .first()
                                .and_then(|&output| dag.artifacts.entities(output).get(port))
                                .ok_or_else(|| lacks(name))?;
                            argument.push(ArgPart::Text(value.to_owned()));
                            continue;
                        }
                        Some(Slot::Output(index)) => job.outputs.get(index),
                        Some(Slot::Input(index)) => job.inputs.get(index).and_then(|a| a.first()),
                        None => None,
                    };
                    let artifact = *artifact.ok_or_else(|| lacks(name))?;
                    if let (Facet::Prefix, Some(Slot::Output(index))) = (facet, &slot) {
                        argument.push(ArgPart::Dir(artifact));
                        argument.push(ArgPart::Text("/".to_owned()));
                        argument.push(ArgPart::Stem {
                            artifact,
                            extension_len: operation.outputs[*index]
                                .ending
                                .as_deref()
                                .map_or(0, str::len),
                        });
                        continue;
                    }
                    argument.push(match (facet, slot) {
                        (Facet::Dimension(dimension), _) => value(artifact, dimension, name)?,
                        (Facet::Dir, _) => ArgPart::Dir(artifact),

                        (Facet::Stem, Some(Slot::Output(index))) => ArgPart::Stem {
                            artifact,
                            extension_len: operation.outputs[index]
                                .extension
                                .as_deref()
                                .map_or(0, str::len),
                        },
                        _ => path(&artifact)?,
                    });
                }
            }
        }
        arguments.push(argument);
    }
    Ok(arguments)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindError {
    Path(PathError),
    Command(CommandError),
    Dag(String),
}

impl From<PathError> for BindError {
    fn from(error: PathError) -> Self {
        Self::Path(error)
    }
}

impl From<CommandError> for BindError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(error) => error.fmt(f),
            Self::Command(error) => error.fmt(f),
            Self::Dag(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for BindError {}
