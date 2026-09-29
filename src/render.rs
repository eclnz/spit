use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::model::{
    ArtifactInstance, ArtifactReport, Gap, Job, OperationDef, Pipeline, ResolvedDag,
};
use crate::paths::{bound_paths, check_rules, error, PathError};
use crate::types::TypeExpr;

pub fn render_dag(dag: &ResolvedDag) -> String {
    write_jobs(dag, |_| Ok(None), |_| None).expect("rendering without ports cannot fail")
}

/// A versioned, logical DAG for consumers that do not need paths or commands.
pub fn render_dag_json(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<String, PathError> {
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let produced: BTreeSet<_> = dag
        .jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .map(ArtifactInstance::key)
        .collect();
    let external: BTreeMap<_, _> = dag
        .jobs
        .iter()
        .flat_map(Job::input_artifacts)
        .filter(|artifact| !produced.contains(&artifact.key()))
        .map(|artifact| (artifact.key(), artifact))
        .collect();

    let mut output = String::from("{\"version\":1,\"external_inputs\":[");
    for (index, artifact) in external.values().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write_json_artifact(&mut output, artifact);
    }
    output.push_str("],\"jobs\":[");
    for (index, job) in dag.jobs.iter().enumerate() {
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
        if index > 0 {
            output.push(',');
        }
        write!(output, "{{\"id\":{},\"operation\":", job.id).unwrap();
        write_json_string(&mut output, &job.operation);
        output.push_str(",\"stage\":[");
        if let Some(stage) = &job.stage {
            for (index, component) in stage.split('/').enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_json_string(&mut output, component);
            }
        }
        output.push(']');
        output.push_str(",\"inputs\":{");
        for (port_index, artifacts) in job.inputs.iter().enumerate() {
            if port_index > 0 {
                output.push(',');
            }
            write_json_string(&mut output, &operation.inputs[port_index].name);
            output.push_str(":[");
            for (artifact_index, artifact) in artifacts.iter().enumerate() {
                if artifact_index > 0 {
                    output.push(',');
                }
                write_json_artifact(&mut output, artifact);
            }
            output.push(']');
        }
        output.push_str("},\"outputs\":{");
        for (port_index, artifact) in job.outputs.iter().enumerate() {
            if port_index > 0 {
                output.push(',');
            }
            write_json_string(&mut output, &operation.outputs[port_index].name);
            output.push(':');
            write_json_artifact(&mut output, artifact);
        }
        output.push_str("},\"depends_on\":[");
        for (dependency_index, dependency) in job.dependencies.iter().enumerate() {
            if dependency_index > 0 {
                output.push(',');
            }
            write!(output, "{dependency}").unwrap();
        }
        output.push_str("]}");
    }
    output.push_str("]}\n");
    Ok(output)
}

fn write_json_artifact(output: &mut String, artifact: &ArtifactInstance) {
    output.push_str("{\"product\":");
    write_json_string(output, &artifact.product);
    output.push_str(",\"entities\":{");
    for (index, (dimension, value)) in artifact.entities.0.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write_json_string(output, dimension);
        output.push(':');
        write_json_string(output, value);
    }
    output.push_str("},\"type\":");
    write_json_type(output, &artifact.artifact_type);
    output.push('}');
}

fn write_json_type(output: &mut String, artifact_type: &TypeExpr) {
    match artifact_type {
        TypeExpr::Unknown => output.push_str("null"),
        TypeExpr::Variable(name) => {
            output.push_str("{\"variable\":");
            write_json_string(output, name);
            output.push('}');
        }
        TypeExpr::Named(name) => {
            output.push_str("{\"name\":");
            write_json_string(output, name);
            output.push_str(",\"args\":[]}");
        }
        TypeExpr::Applied { constructor, args } => {
            output.push_str("{\"name\":");
            write_json_string(output, constructor);
            output.push_str(",\"args\":[");
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_json_type(output, arg);
            }
            output.push_str("]}");
        }
    }
}

fn write_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c < ' ' => write!(output, "\\u{:04x}", u32::from(c)).unwrap(),
            c => output.push(c),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod json_tests {
    use super::{write_json_string, write_json_type};
    use crate::types::TypeExpr;

    #[test]
    fn escapes_names_and_entity_values() {
        let mut output = String::new();
        write_json_string(&mut output, "a\"\\\n\t\u{0001}é");
        assert_eq!(output, "\"a\\\"\\\\\\n\\t\\u0001é\"");
    }

    #[test]
    fn writes_nested_and_partially_unknown_types() {
        let artifact_type = TypeExpr::applied(
            "MRI",
            vec![
                TypeExpr::applied("Pair", vec![TypeExpr::named("T1w"), TypeExpr::Unknown]),
                TypeExpr::named("Diffusion"),
            ],
        );
        let mut output = String::new();
        write_json_type(&mut output, &artifact_type);
        assert_eq!(output, "{\"name\":\"MRI\",\"args\":[{\"name\":\"Pair\",\"args\":[{\"name\":\"T1w\",\"args\":[]},null]},{\"name\":\"Diffusion\",\"args\":[]}]}");
    }
}

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
                "{}  (job {}: {}{})",
                render_typed_artifact(dag, artifact),
                job.id,
                job.operation,
                in_stage(job.stage.as_deref())
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
            writeln!(
                output,
                "  {rendered}  ({}{})",
                job.operation,
                in_stage(job.stage.as_deref())
            )
            .unwrap();
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
    check_rules(pipeline, dag)?;
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
        if let Some(stage) = &job.stage {
            writeln!(output, "  stage: {stage}").unwrap();
        }
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

fn in_stage(stage: Option<&str>) -> String {
    stage.map_or_else(String::new, |stage| format!(", stage {stage}"))
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
