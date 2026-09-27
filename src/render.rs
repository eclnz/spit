use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::model::{
    ArtifactInstance, ArtifactReport, Gap, Job, OperationDef, Pipeline, ResolvedDag,
};
use crate::paths::{bound_paths, error, inspect_paths, PathError};
use crate::types::TypeExpr;

pub fn render_dag(dag: &ResolvedDag) -> String {
    write_jobs(dag, |_| Ok(None), |_| None).expect("rendering without ports cannot fail")
}

/// List every artifact that can be produced, then every one that cannot with
/// the reasons why, then the coverage rules the sources fail.
pub fn render_artifacts(report: &ArtifactReport) -> String {
    let dag = &report.dag;
    let held_back: BTreeSet<_> = report
        .coverage
        .iter()
        .flat_map(|gap| &gap.sources)
        .collect();
    let mut complete = Vec::new();
    for source in &report.sources {
        if !held_back.contains(source) {
            complete.push(format!("{}  (source)", render_typed_artifact(dag, source)));
        }
    }
    for job in &dag.jobs {
        for artifact in &job.outputs {
            complete.push(format!(
                "{}  (job {}: {})",
                render_typed_artifact(dag, artifact),
                job.id,
                job.operation
            ));
        }
    }
    let mut output = String::new();
    writeln!(output, "Complete artifacts: {}", complete.len()).unwrap();
    for line in complete {
        writeln!(output, "  {line}").unwrap();
    }

    let incomplete: usize = report.incomplete.iter().map(|job| job.outputs.len()).sum();
    writeln!(output, "\nIncomplete artifacts: {incomplete}").unwrap();
    for job in &report.incomplete {
        for artifact in &job.outputs {
            let rendered = render_typed_artifact(dag, artifact);
            writeln!(output, "  {rendered}  ({})", job.operation).unwrap();
        }
        for gap in &job.gaps {
            match gap {
                Gap::Unmatched(error) => writeln!(output, "    - {error}").unwrap(),
                Gap::Blocked { port, artifact } => {
                    let reason = if held_back.contains(artifact) {
                        "a coverage gap holds back"
                    } else {
                        "cannot be produced"
                    };
                    writeln!(
                        output,
                        "    - input `{port}` needs {}, which {reason}",
                        render_artifact(dag, artifact)
                    )
                    .unwrap();
                }
            }
        }
    }

    if !report.coverage.is_empty() {
        writeln!(output, "\nCoverage gaps: {}", report.coverage.len()).unwrap();
        for gap in &report.coverage {
            writeln!(output, "  {}", gap.error).unwrap();
            if !gap.sources.is_empty() {
                let sources: Vec<_> = gap
                    .sources
                    .iter()
                    .map(|source| render_artifact(dag, source))
                    .collect();
                writeln!(output, "    holds back: {}", sources.join(", ")).unwrap();
            }
        }
    }
    output
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
    let ports = |job: &Job| {
        let operation = operations.get(job.operation.as_str()).ok_or_else(|| {
            error(format!(
                "unknown operation `{}` in resolved DAG",
                job.operation
            ))
        })?;
        Ok(Some(*operation))
    };
    write_jobs(dag, ports, |artifact| {
        paths.get(&artifact.key()).map(String::as_str)
    })
}

/// Shared job listing. `operation` gives the port names to label inputs and
/// outputs with, and `path` adds a path line beneath an artifact; either may
/// return `None` to omit it.
fn write_jobs<'a>(
    dag: &ResolvedDag,
    operation: impl Fn(&Job) -> Result<Option<&'a OperationDef>, PathError>,
    path: impl Fn(&ArtifactInstance) -> Option<&'a str>,
) -> Result<String, PathError> {
    let write_artifact = |output: &mut String, port: Option<&str>, artifact: &ArtifactInstance| {
        let rendered = render_typed_artifact(dag, artifact);
        match port {
            Some(port) => writeln!(output, "    {port}: {rendered}").unwrap(),
            None => writeln!(output, "    {rendered}").unwrap(),
        }
        if let Some(path) = path(artifact) {
            writeln!(output, "      path: {path}").unwrap();
        }
    };
    let mut output = String::new();
    for (index, job) in dag.jobs.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let operation = operation(job)?;
        writeln!(output, "Job {}", job.id).unwrap();
        writeln!(output, "  operation: {}", job.operation).unwrap();
        writeln!(output, "  inputs:").unwrap();
        for (port_index, artifacts) in job.inputs.iter().enumerate() {
            let port = operation
                .map(|operation| {
                    operation.inputs.get(port_index).ok_or_else(|| {
                        error(format!(
                            "job {} has more inputs than operation ports",
                            job.id
                        ))
                    })
                })
                .transpose()?;
            for artifact in artifacts {
                write_artifact(&mut output, port.map(|port| port.name.as_str()), artifact);
            }
        }
        if let [artifact] = job.outputs.as_slice() {
            writeln!(output, "  output:").unwrap();
            write_artifact(&mut output, None, artifact);
        } else {
            writeln!(output, "  outputs:").unwrap();
            for (port_index, artifact) in job.outputs.iter().enumerate() {
                let port = operation.and_then(|operation| operation.outputs.get(port_index));
                write_artifact(&mut output, port.map(|port| port.name.as_str()), artifact);
            }
        }
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
