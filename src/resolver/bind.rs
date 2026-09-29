//! Bind a resolved DAG for a backend: give each artifact its path and expand
//! each job's commands into arguments, so the result needs no pipeline.

use std::collections::BTreeMap;

use crate::command::{slot, validate_commands, Slot};
use crate::model::{
    ArtifactInstance, ArtifactKey, Cardinality, CommandRole, Job, OperationDef, Pipeline,
    ResolvedDag,
};
use crate::paths::{bound_paths, check_rules, error, PathError};
use crate::spitdag::{ArgPart, Argument, BoundArtifact, BoundDag, BoundJob};
use crate::template::Part;

/// Bind every artifact of `dag` to its path and expand every job's command
/// and `verify` commands. A job whose operation has no command keeps none;
/// a backend that runs jobs reports it.
pub fn bind_dag(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<BoundDag, PathError> {
    check_rules(pipeline, dag)?;
    validate_commands(pipeline)?;
    let paths = bound_paths(pipeline, dag)?;
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let artifact = |artifact: &ArtifactInstance| {
        let dimensions = &dag.product_dimensions[&artifact.product];
        BoundArtifact {
            product: artifact.product.clone(),
            entities: dimensions
                .iter()
                .filter_map(|dimension| {
                    let value = artifact.entities.0.get(dimension)?;
                    Some((dimension.clone(), value.clone()))
                })
                .collect(),
            artifact_type: artifact.artifact_type.clone(),
            path: paths[&artifact.key()].clone(),
        }
    };
    let mut jobs = Vec::new();
    for job in &dag.jobs {
        let operation = operations.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
        if job.inputs.len() != operation.inputs.len()
            || job.outputs.len() != operation.outputs.len()
        {
            return Err(error(format!(
                "job {} has different ports from operation `{}`",
                job.id, job.operation
            )));
        }
        let commands = |role: CommandRole| {
            pipeline
                .commands
                .iter()
                .filter(move |command| command.operation == job.operation && command.role == role)
                .map(|command| expand(command.template.arguments(), operation, job, &paths))
        };
        jobs.push(BoundJob {
            id: job.id,
            operation: job.operation.clone(),
            stage: job.stage.clone(),
            inputs: operation
                .inputs
                .iter()
                .zip(&job.inputs)
                .map(|(port, artifacts)| {
                    (port.name.clone(), artifacts.iter().map(artifact).collect())
                })
                .collect(),
            outputs: operation
                .outputs
                .iter()
                .zip(&job.outputs)
                .map(|(port, output)| (port.name.clone(), artifact(output)))
                .collect(),
            depends_on: job.dependencies.clone(),
            command: commands(CommandRole::Run).next().transpose()?,
            verify: commands(CommandRole::Verify).collect::<Result<_, _>>()?,
        });
    }
    Ok(BoundDag { jobs })
}

/// A command's arguments for one job. A many input's placeholder, which is a
/// whole argument, becomes one argument per artifact.
fn expand(
    template: &[Vec<Part>],
    operation: &OperationDef,
    job: &Job,
    paths: &BTreeMap<ArtifactKey, String>,
) -> Result<Vec<Argument>, PathError> {
    let path = |artifact: &ArtifactInstance| ArgPart::Path(paths[&artifact.key()].clone());
    let mut arguments = Vec::new();
    for parts in template {
        if let [Part::Placeholder(name)] = parts.as_slice() {
            if let Some(Slot::Input(index)) = slot(operation, name) {
                if operation.inputs[index].cardinality == Cardinality::Many {
                    arguments.extend(
                        job.inputs[index]
                            .iter()
                            .map(|artifact| vec![path(artifact)]),
                    );
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
                    let artifact = artifact
                        .ok_or_else(|| error(format!("job {} lacks `{{{name}}}`", job.id)))?;
                    argument.push(path(artifact));
                }
            }
        }
        arguments.push(argument);
    }
    Ok(arguments)
}
