//! Bind a resolved DAG for a backend: give each artifact its path and expand
//! each job's commands into arguments, so the result needs no pipeline.

use std::collections::BTreeMap;

use std::fmt;

use crate::command::{facet, slot, validate_commands, CommandError, Facet, Slot};
use crate::model::{
    ArtifactId, Cardinality, CommandRole, Job, OperationDef, Pipeline, ResolvedDag,
};
use crate::paths::{bound_paths, check_rules, BoundPaths, PathError};
use crate::spitdag::{ArgPart, Argument, BoundDag, BoundJob};
use crate::template::Part;

/// Bind every artifact of `dag` to its path and expand every job's command
/// and `verify` commands. A job whose operation has no command keeps none;
/// a backend that runs jobs reports it.
pub fn bind_dag(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<BoundDag, BindError> {
    // Keep in step with `bind_dag_with`, which skips the steps up to the
    // paths' binding because `validate_bound_source_files` took them.
    check_rules(pipeline, dag)?;
    validate_commands(pipeline)?;
    let paths = bound_paths(pipeline, dag)?;
    bind_jobs(pipeline, dag, paths)
}

/// As [`bind_dag`], with the paths [`validate_bound_source_files`] bound for
/// the same pipeline and DAG, which checked their rules.
///
/// Keep in step with [`bind_dag`] and `validate_bound_source_files`: this
/// leaves out `bind_dag`'s rule check and path binding because that
/// function did both, so a check `bind_dag` gains before its paths are
/// bound belongs there or here as well.
///
/// [`validate_bound_source_files`]: crate::validate_bound_source_files
pub fn bind_dag_with(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    paths: BoundPaths,
) -> Result<BoundDag, BindError> {
    validate_commands(pipeline)?;
    bind_jobs(pipeline, dag, paths.0)
}

/// Every job of `dag` with its ports named and its commands expanded, over
/// the path `paths` holds for each artifact.
fn bind_jobs(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    paths: Vec<Option<String>>,
) -> Result<BoundDag, BindError> {
    let operations: BTreeMap<&str, &OperationDef> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let jobs = dag
        .jobs
        .iter()
        .map(|job| bind_job(pipeline, &operations, dag, &paths, job))
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
    Ok(BoundDag::new(
        dag.artifacts.clone(),
        paths,
        dimensions,
        jobs,
    ))
}

/// `job` with its ports named and its commands expanded. `bound_paths`
/// binds every artifact of every job, so each has a path.
fn bind_job(
    pipeline: &Pipeline,
    operations: &BTreeMap<&str, &OperationDef>,
    dag: &ResolvedDag,
    paths: &[Option<String>],
    job: &Job,
) -> Result<BoundJob, BindError> {
    let operation = operations
        .get(job.operation.as_str())
        .copied()
        .ok_or_else(|| {
            BindError::Dag(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
    if job.inputs.len() != operation.inputs.len() || job.outputs.len() != operation.outputs.len() {
        return Err(BindError::Dag(format!(
            "job {} has different ports from operation `{}`",
            job.id, job.operation
        )));
    }
    let bound = |id: ArtifactId| match &paths[id.index()] {
        Some(_) => Ok(id),
        None => Err(BindError::Dag(format!(
            "no path is bound for `{}`",
            dag.artifact(id)
        ))),
    };
    let mut inputs = Vec::with_capacity(job.inputs.len());
    for (port, artifacts) in operation.inputs.iter().zip(&job.inputs) {
        let artifacts = artifacts
            .iter()
            .map(|&artifact| bound(artifact))
            .collect::<Result<_, _>>()?;
        inputs.push((port.name.clone(), artifacts));
    }
    let mut outputs = Vec::with_capacity(job.outputs.len());
    for (port, &output) in operation.outputs.iter().zip(&job.outputs) {
        outputs.push((port.name.clone(), bound(output)?));
    }
    let commands = |role: CommandRole| {
        pipeline
            .commands
            .iter()
            .filter(move |command| command.operation == job.operation && command.role == role)
            .map(|command| expand(command.template.arguments(), operation, job))
    };
    Ok(BoundJob {
        id: job.id,
        operation: job.operation.clone(),
        stage: job.stage.clone(),
        inputs,
        outputs,
        depends_on: job.dependencies.clone(),
        command: commands(CommandRole::Run).next().transpose()?,
        verify: commands(CommandRole::Verify).collect::<Result<_, _>>()?,
    })
}

/// A command's arguments for one job. A many input's placeholder, which is a
/// whole argument, becomes one argument per artifact.
fn expand(
    template: &[Vec<Part>],
    operation: &OperationDef,
    job: &Job,
) -> Result<Vec<Argument>, BindError> {
    // Every artifact of the job has a path, which `bind_job` checks first.
    let path = |artifact: &ArtifactId| Ok::<_, BindError>(ArgPart::Path(*artifact));
    let lacks = |name: &str| CommandError::new(format!("job {} lacks `{{{name}}}`", job.id));
    let mut arguments = Vec::new();
    for parts in template {
        if let [Part::Placeholder(name)] = parts.as_slice() {
            if let Some(Slot::Input(index)) = slot(operation, facet(name).0) {
                if operation.inputs[index].cardinality == Cardinality::Many {
                    let artifacts = job.inputs.get(index).ok_or_else(|| lacks(name))?;
                    for artifact in artifacts {
                        arguments.push(vec![path(artifact)?]);
                    }
                    continue;
                }
            }
        }
        let mut argument = Vec::new();
        for part in parts {
            match part {
                Part::Literal(value) => argument.push(ArgPart::Text(value.clone())),
                Part::Placeholder(name) => {
                    let (port, facet) = facet(name);
                    let slot = slot(operation, port);
                    let artifact = match slot {
                        Some(Slot::Output(index)) => job.outputs.get(index),
                        Some(Slot::Input(index)) => job.inputs.get(index).and_then(|a| a.first()),
                        // `validate_commands` rejects unknown placeholders.
                        None => None,
                    };
                    let artifact = *artifact.ok_or_else(|| lacks(name))?;
                    // `validate_commands` allows `.dir` and `.stem` only on
                    // an output, and `.stem` only with an extension.
                    argument.push(match (facet, slot) {
                        (Ok(Facet::Dir), _) => ArgPart::Dir(artifact),
                        (Ok(Facet::Stem), Some(Slot::Output(index))) => ArgPart::Stem {
                            artifact,
                            extension: operation.outputs[index]
                                .extension
                                .clone()
                                .unwrap_or_default(),
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

/// Why a resolved DAG cannot be bound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindError {
    /// A path rule is missing or invalid, or binds two artifacts to one file.
    Path(PathError),
    /// A command names something its operation lacks.
    Command(CommandError),
    /// The DAG does not match the pipeline, as when it was resolved from
    /// another.
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
