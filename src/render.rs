//! Text reports of a resolved DAG: its jobs, and what can and cannot be made.

use std::collections::BTreeSet;
use std::fmt::Write;

use crate::model::{ArtifactInstance, ArtifactReport, Gap, ResolvedDag};
use crate::types::TypeExpr;

/// The jobs as text, without paths; a bound DAG adds ports and paths.
pub fn render_dag(dag: &ResolvedDag) -> String {
    write_jobs(dag)
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

fn write_jobs(dag: &ResolvedDag) -> String {
    let mut output = String::new();
    for (index, job) in dag.jobs.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        writeln!(output, "Job {}", job.id).unwrap();
        if let Some(stage) = &job.stage {
            writeln!(output, "  stage: {stage}").unwrap();
        }
        writeln!(output, "  operation: {}", job.operation).unwrap();
        writeln!(output, "  inputs:").unwrap();
        for artifact in job.input_artifacts() {
            writeln!(output, "    {}", render_typed_artifact(dag, artifact)).unwrap();
        }
        writeln!(
            output,
            "  {}:",
            if job.outputs.len() == 1 {
                "output"
            } else {
                "outputs"
            }
        )
        .unwrap();
        for artifact in &job.outputs {
            writeln!(output, "    {}", render_typed_artifact(dag, artifact)).unwrap();
        }
        if !job.dependencies.is_empty() {
            let dependencies: Vec<_> = job.dependencies.iter().map(ToString::to_string).collect();
            writeln!(output, "  depends_on: {}", dependencies.join(", ")).unwrap();
        }
    }
    output
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
