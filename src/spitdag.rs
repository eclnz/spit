//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::types::TypeExpr;

/// The schema version a `.spitdag` is written with.
pub const SPITDAG_VERSION: u64 = 2;

/// A resolved DAG with its paths bound and its commands expanded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundDag {
    pub jobs: Vec<BoundJob>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJob {
    pub id: usize,
    pub operation: String,
    /// The stage of the step that made this job, as `outer/inner`.
    pub stage: Option<String>,
    /// Each input port and its artifacts, in port order.
    pub inputs: Vec<(String, Vec<BoundArtifact>)>,
    /// Each output port and its artifact, in port order.
    pub outputs: Vec<(String, BoundArtifact)>,
    pub depends_on: Vec<usize>,
    /// The command that makes the outputs; `None` when the operation has none.
    pub command: Option<Vec<Argument>>,
    /// Commands that check the inputs before the job runs.
    pub verify: Vec<Vec<Argument>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundArtifact {
    pub product: String,
    /// Each dimension and value, in the product's declared order.
    pub entities: Vec<(String, String)>,
    pub artifact_type: TypeExpr,
    /// The file, relative to the dataset root.
    pub path: String,
}

/// One command-line argument: literal text and artifact paths, joined.
pub type Argument = Vec<ArgPart>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgPart {
    Text(String),
    /// A file relative to the dataset root.
    Path(String),
}

impl BoundArtifact {
    /// Its product and bindings, as `image[sub=1,ses=2]`.
    pub fn identity(&self) -> String {
        let bindings: Vec<_> = self
            .entities
            .iter()
            .map(|(dimension, value)| format!("{dimension}={value}"))
            .collect();
        format!("{}[{}]", self.product, bindings.join(","))
    }
}

impl BoundJob {
    /// Every input artifact, in port order.
    pub fn input_artifacts(&self) -> impl Iterator<Item = &BoundArtifact> {
        self.inputs.iter().flat_map(|(_, artifacts)| artifacts)
    }
}

impl BoundDag {
    /// The inputs no job here makes, each once, by path: sources, and the
    /// outputs of stages left out.
    pub fn external_inputs(&self) -> Vec<&BoundArtifact> {
        let produced: BTreeSet<_> = self
            .jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .map(|(_, artifact)| artifact.path.as_str())
            .collect();
        let mut external = BTreeMap::new();
        for artifact in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            if !produced.contains(artifact.path.as_str()) {
                external.entry(artifact.path.as_str()).or_insert(artifact);
            }
        }
        external.into_values().collect()
    }

    /// The `.spitdag` document.
    pub fn to_json(&self) -> String {
        let mut output = format!("{{\"version\":{SPITDAG_VERSION},\"external_inputs\":[");
        for (index, artifact) in self.external_inputs().into_iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write_artifact(&mut output, artifact);
        }
        output.push_str("],\"jobs\":[");
        for (index, job) in self.jobs.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write_job(&mut output, job);
        }
        output.push_str("]}\n");
        output
    }
}

/// The jobs as text, each artifact with its path when `paths` is set.
pub fn render_bound_dag(dag: &BoundDag, paths: bool) -> String {
    let mut output = String::new();
    let write_artifact = |output: &mut String, port: Option<&str>, artifact: &BoundArtifact| {
        let mut rendered = artifact.identity();
        if artifact.artifact_type != TypeExpr::Unknown {
            write!(rendered, " : {}", artifact.artifact_type).unwrap();
        }
        match port {
            Some(port) => writeln!(output, "    {port}: {rendered}").unwrap(),
            None => writeln!(output, "    {rendered}").unwrap(),
        }
        if paths {
            writeln!(output, "      path: {}", artifact.path).unwrap();
        }
    };
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
        for (port, artifacts) in &job.inputs {
            for artifact in artifacts {
                write_artifact(&mut output, Some(port), artifact);
            }
        }
        if let [(_, artifact)] = job.outputs.as_slice() {
            writeln!(output, "  output:").unwrap();
            write_artifact(&mut output, None, artifact);
        } else {
            writeln!(output, "  outputs:").unwrap();
            for (port, artifact) in &job.outputs {
                write_artifact(&mut output, Some(port), artifact);
            }
        }
        if !job.depends_on.is_empty() {
            let dependencies: Vec<_> = job.depends_on.iter().map(ToString::to_string).collect();
            writeln!(output, "  depends_on: {}", dependencies.join(", ")).unwrap();
        }
    }
    output
}

fn write_job(output: &mut String, job: &BoundJob) {
    write!(output, "{{\"id\":{},\"operation\":", job.id).unwrap();
    json::write_string(output, &job.operation);
    output.push_str(",\"stage\":[");
    if let Some(stage) = &job.stage {
        for (index, component) in stage.split('/').enumerate() {
            if index > 0 {
                output.push(',');
            }
            json::write_string(output, component);
        }
    }
    output.push_str("],\"inputs\":{");
    for (index, (port, artifacts)) in job.inputs.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        json::write_string(output, port);
        output.push_str(":[");
        for (index, artifact) in artifacts.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            write_artifact(output, artifact);
        }
        output.push(']');
    }
    output.push_str("},\"outputs\":{");
    for (index, (port, artifact)) in job.outputs.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        json::write_string(output, port);
        output.push(':');
        write_artifact(output, artifact);
    }
    output.push_str("},\"depends_on\":[");
    let dependencies: Vec<_> = job.depends_on.iter().map(ToString::to_string).collect();
    output.push_str(&dependencies.join(","));
    output.push_str("],\"command\":");
    match &job.command {
        Some(command) => write_command(output, command),
        None => output.push_str("null"),
    }
    output.push_str(",\"verify\":[");
    for (index, command) in job.verify.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write_command(output, command);
    }
    output.push_str("]}");
}

/// An argument is an array of parts: a string for text, `{"path": ...}` for
/// a file.
fn write_command(output: &mut String, command: &[Argument]) {
    output.push('[');
    for (index, argument) in command.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('[');
        for (index, part) in argument.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            match part {
                ArgPart::Text(text) => json::write_string(output, text),
                ArgPart::Path(path) => {
                    output.push_str("{\"path\":");
                    json::write_string(output, path);
                    output.push('}');
                }
            }
        }
        output.push(']');
    }
    output.push(']');
}

fn write_artifact(output: &mut String, artifact: &BoundArtifact) {
    output.push_str("{\"product\":");
    json::write_string(output, &artifact.product);
    output.push_str(",\"entities\":{");
    for (index, (dimension, value)) in artifact.entities.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        json::write_string(output, dimension);
        output.push(':');
        json::write_string(output, value);
    }
    output.push_str("},\"type\":");
    write_type(output, &artifact.artifact_type);
    output.push_str(",\"path\":");
    json::write_string(output, &artifact.path);
    output.push('}');
}

fn write_type(output: &mut String, artifact_type: &TypeExpr) {
    match artifact_type {
        TypeExpr::Unknown => output.push_str("null"),
        TypeExpr::Variable(name) => {
            output.push_str("{\"variable\":");
            json::write_string(output, name);
            output.push('}');
        }
        TypeExpr::Named(name) => {
            output.push_str("{\"name\":");
            json::write_string(output, name);
            output.push_str(",\"args\":[]}");
        }
        TypeExpr::Applied { constructor, args } => {
            output.push_str("{\"name\":");
            json::write_string(output, constructor);
            output.push_str(",\"args\":[");
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_type(output, arg);
            }
            output.push_str("]}");
        }
    }
}

/// Just enough JSON for a `.spitdag`.
mod json {
    use std::fmt::Write;

    pub(super) fn write_string(output: &mut String, value: &str) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(product: &str, path: &str) -> BoundArtifact {
        BoundArtifact {
            product: product.into(),
            entities: vec![
                ("sub".into(), "1".into()),
                ("run".into(), "a\"\\\n\u{1}é".into()),
            ],
            artifact_type: TypeExpr::applied(
                "MRI",
                vec![
                    TypeExpr::applied("Pair", vec![TypeExpr::named("T1w"), TypeExpr::Unknown]),
                    TypeExpr::named("Diffusion"),
                ],
            ),
            path: path.into(),
        }
    }

    #[test]
    fn a_spitdag_is_written_with_its_version_and_escapes() {
        let dag = BoundDag {
            jobs: vec![
                BoundJob {
                    id: 1,
                    operation: "clean".into(),
                    stage: Some("prep/denoise".into()),
                    inputs: vec![("raw".into(), vec![artifact("raw", "in/1.txt")])],
                    outputs: vec![("output".into(), artifact("clean", "out/1.txt"))],
                    depends_on: vec![],
                    command: Some(vec![
                        vec![ArgPart::Text("tool".into())],
                        vec![
                            ArgPart::Text("--in=".into()),
                            ArgPart::Path("in/1.txt".into()),
                        ],
                    ]),
                    verify: vec![vec![vec![ArgPart::Path("in/1.txt".into())]]],
                },
                BoundJob {
                    id: 2,
                    operation: "mean".into(),
                    stage: None,
                    inputs: vec![("frames".into(), vec![artifact("clean", "out/1.txt")])],
                    outputs: vec![("output".into(), artifact("mean", "out/mean.txt"))],
                    depends_on: vec![1],
                    command: None,
                    verify: vec![],
                },
            ],
        };
        let text = dag.to_json();
        assert!(text.starts_with("{\"version\":2,\"external_inputs\":[{\"product\":\"raw\""));
        assert!(
            text.contains("\"entities\":{\"sub\":\"1\",\"run\":\"a\\\"\\\\\\n\\u0001é\"}"),
            "{text}"
        );
        assert!(text.contains("\"command\":null"), "{text}");
    }
}
