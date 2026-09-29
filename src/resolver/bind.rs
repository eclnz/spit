//! Bind a resolved DAG for a backend: give each artifact its path and expand
//! each job's commands into arguments, so the result needs no pipeline.

use std::collections::BTreeMap;

use std::fmt;

use crate::command::{slot, validate_commands, CommandError, Slot};
use crate::model::{
    ArtifactInstance, ArtifactKey, Cardinality, CommandRole, Job, OperationDef, Pipeline,
    ResolvedDag,
};
use crate::paths::{bound_paths, check_rules, PathError};
use crate::spitdag::{ArgPart, Argument, BoundArtifact, BoundDag, BoundJob};
use crate::template::Part;

/// Bind every artifact of `dag` to its path and expand every job's command
/// and `verify` commands. A job whose operation has no command keeps none;
/// a backend that runs jobs reports it.
pub fn bind_dag(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<BoundDag, BindError> {
    check_rules(pipeline, dag)?;
    validate_commands(pipeline)?;
    let binder = Binder {
        pipeline,
        dag,
        paths: bound_paths(pipeline, dag)?,
        operations: pipeline
            .operations
            .iter()
            .map(|operation| (operation.name.as_str(), operation))
            .collect(),
    };
    let jobs = dag
        .jobs
        .iter()
        .map(|job| binder.job(job))
        .collect::<Result<_, _>>()?;
    Ok(BoundDag { root: None, jobs })
}

/// What binding each job of a DAG needs: every artifact's path, and each
/// operation by name.
struct Binder<'a> {
    pipeline: &'a Pipeline,
    dag: &'a ResolvedDag,
    paths: BTreeMap<ArtifactKey, String>,
    operations: BTreeMap<&'a str, &'a OperationDef>,
}

impl Binder<'_> {
    fn job(&self, job: &Job) -> Result<BoundJob, BindError> {
        let operation = self.operations.get(job.operation.as_str()).ok_or_else(|| {
            BindError::Dag(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
        if job.inputs.len() != operation.inputs.len()
            || job.outputs.len() != operation.outputs.len()
        {
            return Err(BindError::Dag(format!(
                "job {} has different ports from operation `{}`",
                job.id, job.operation
            )));
        }
        let commands = |role: CommandRole| {
            self.pipeline
                .commands
                .iter()
                .filter(move |command| command.operation == job.operation && command.role == role)
                .map(|command| expand(command.template.arguments(), operation, job, &self.paths))
        };
        let inputs = operation
            .inputs
            .iter()
            .zip(&job.inputs)
            .map(|(port, artifacts)| {
                let artifacts = artifacts
                    .iter()
                    .map(|artifact| self.artifact(artifact))
                    .collect::<Result<_, _>>()?;
                Ok((port.name.clone(), artifacts))
            });
        let outputs = (operation.outputs.iter().zip(&job.outputs))
            .map(|(port, output)| Ok((port.name.clone(), self.artifact(output)?)));
        Ok(BoundJob {
            id: job.id,
            operation: job.operation.clone(),
            stage: job.stage.clone(),
            inputs: inputs.collect::<Result<_, BindError>>()?,
            outputs: outputs.collect::<Result<_, BindError>>()?,
            depends_on: job.dependencies.clone(),
            command: commands(CommandRole::Run).next().transpose()?,
            verify: commands(CommandRole::Verify).collect::<Result<_, _>>()?,
        })
    }

    /// `artifact` with its path and its entities in declared order.
    /// `bound_paths` binds every artifact of every job, so each is found.
    fn artifact(&self, artifact: &ArtifactInstance) -> Result<BoundArtifact, BindError> {
        let unbound = || BindError::Dag(format!("no path is bound for `{artifact}`"));
        let dimensions = self
            .dag
            .product_dimensions
            .get(&artifact.product)
            .ok_or_else(unbound)?;
        let path = self.paths.get(&artifact.key()).ok_or_else(unbound)?;
        Ok(BoundArtifact {
            product: artifact.product.clone(),
            entities: dimensions
                .iter()
                .filter_map(|dimension| {
                    let value = artifact.entities.get(dimension)?;
                    Some((dimension.clone(), value.to_owned()))
                })
                .collect(),
            artifact_type: artifact.artifact_type.clone(),
            path: path.clone(),
        })
    }
}

/// A command's arguments for one job. A many input's placeholder, which is a
/// whole argument, becomes one argument per artifact.
fn expand(
    template: &[Vec<Part>],
    operation: &OperationDef,
    job: &Job,
    paths: &BTreeMap<ArtifactKey, String>,
) -> Result<Vec<Argument>, BindError> {
    let path = |artifact: &ArtifactInstance| {
        let path = paths
            .get(&artifact.key())
            .ok_or_else(|| BindError::Dag(format!("no path is bound for `{artifact}`")))?;
        Ok::<_, BindError>(ArgPart::Path(path.clone()))
    };
    let lacks = |name: &str| CommandError::new(format!("job {} lacks `{{{name}}}`", job.id));
    let mut arguments = Vec::new();
    for parts in template {
        if let [Part::Placeholder(name)] = parts.as_slice() {
            if let Some(Slot::Input(index)) = slot(operation, name) {
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
                    let artifact = match slot(operation, name) {
                        Some(Slot::Output(index)) => job.outputs.get(index),
                        Some(Slot::Input(index)) => job.inputs.get(index).and_then(|a| a.first()),
                        // `validate_commands` rejects unknown placeholders.
                        None => None,
                    };
                    let artifact = artifact.ok_or_else(|| lacks(name))?;
                    argument.push(path(artifact)?);
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
