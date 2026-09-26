use std::collections::BTreeMap;
use std::fmt::Write;

use crate::model::{ArtifactInstance, Cardinality, Job, Pipeline, ResolvedDag};
use crate::paths::{bound_paths, error, inspect_paths, key, PathError};
use crate::types::TypeExpr;

pub fn render_dag(dag: &ResolvedDag) -> String {
    write_jobs(dag, |_, _| Ok(None), |_| None).expect("rendering without ports cannot fail")
}

/// Inspect the resolved jobs and bound paths before expanding any commands.
pub fn render_bound_dag(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<String, PathError> {
    inspect_paths(pipeline)?.validate(false)?;
    let paths = bound_paths(pipeline, dag)?;
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let port_name = |job: &Job, input_index: usize| {
        let operation = operations.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
        let port = match operation.inputs.as_slice() {
            [port] if port.cardinality == Cardinality::Many => port,
            ports => ports.get(input_index).ok_or_else(|| {
                error(format!(
                    "job {} has more inputs than operation ports",
                    job.id
                ))
            })?,
        };
        Ok(Some(port.name.as_str()))
    };
    write_jobs(dag, port_name, |artifact| {
        paths.get(&key(artifact)).map(String::as_str)
    })
}

/// Shared job listing. `port_name` labels each input and `path` adds a path
/// line beneath an artifact; either may return `None` to omit it.
fn write_jobs<'a>(
    dag: &ResolvedDag,
    port_name: impl Fn(&Job, usize) -> Result<Option<&'a str>, PathError>,
    path: impl Fn(&ArtifactInstance) -> Option<&'a str>,
) -> Result<String, PathError> {
    let write_path = |output: &mut String, artifact: &ArtifactInstance| {
        if let Some(path) = path(artifact) {
            writeln!(output, "      path: {path}").unwrap();
        }
    };
    let mut output = String::new();
    for (index, job) in dag.jobs.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        writeln!(output, "Job {}", job.id).unwrap();
        writeln!(output, "  operation: {}", job.operation).unwrap();
        writeln!(output, "  inputs:").unwrap();
        for (input_index, input) in job.inputs.iter().enumerate() {
            let artifact = render_typed_artifact(dag, input);
            match port_name(job, input_index)? {
                Some(port) => writeln!(output, "    {port}: {artifact}").unwrap(),
                None => writeln!(output, "    {artifact}").unwrap(),
            }
            write_path(&mut output, input);
        }
        writeln!(output, "  output:").unwrap();
        writeln!(output, "    {}", render_typed_artifact(dag, &job.output)).unwrap();
        write_path(&mut output, &job.output);
        if !job.dependencies.is_empty() {
            let dependencies: Vec<_> = job.dependencies.iter().map(ToString::to_string).collect();
            writeln!(output, "  depends_on: {}", dependencies.join(", ")).unwrap();
        }
    }
    Ok(output)
}

fn render_typed_artifact(dag: &ResolvedDag, artifact: &ArtifactInstance) -> String {
    let identity = render_artifact(dag, artifact);
    if artifact.artifact_type == TypeExpr::Unknown {
        identity
    } else {
        format!("{identity} : {}", artifact.artifact_type)
    }
}

fn render_artifact(dag: &ResolvedDag, artifact: &ArtifactInstance) -> String {
    let Some(dimensions) = dag.product_dimensions.get(&artifact.product) else {
        return artifact.to_string();
    };
    let binding = dimensions
        .iter()
        .filter_map(|dimension| {
            artifact
                .entities
                .0
                .get(dimension)
                .map(|value| format!("{dimension}={value}"))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{}[{binding}]", artifact.product)
}
