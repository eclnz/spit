use std::fmt::Write;

use crate::model::{ArtifactInstance, ResolvedDag};
use crate::types::TypeExpr;

pub fn render_dag(dag: &ResolvedDag) -> String {
    let mut output = String::new();
    for (index, job) in dag.jobs.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        writeln!(output, "Job {}", job.id).unwrap();
        writeln!(output, "  operation: {}", job.operation).unwrap();
        writeln!(output, "  inputs:").unwrap();
        for input in &job.inputs {
            writeln!(output, "    {}", render_typed_artifact(dag, input)).unwrap();
        }
        writeln!(output, "  output:").unwrap();
        writeln!(output, "    {}", render_typed_artifact(dag, &job.output)).unwrap();
        if !job.dependencies.is_empty() {
            let dependencies: Vec<_> = job.dependencies.iter().map(ToString::to_string).collect();
            writeln!(output, "  depends_on: {}", dependencies.join(", ")).unwrap();
        }
    }
    output
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
