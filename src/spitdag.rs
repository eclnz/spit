//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::json::Json;
use crate::types::TypeExpr;

/// The schema version a `.spitdag` is written with.
pub const SPITDAG_VERSION: usize = 2;

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
        let document = Json::object([
            ("version", Json::Number(SPITDAG_VERSION)),
            (
                "external_inputs",
                Json::array(self.external_inputs().into_iter().map(artifact_json)),
            ),
            ("jobs", Json::array(self.jobs.iter().map(job_json))),
        ]);
        format!("{document}\n")
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

fn job_json(job: &BoundJob) -> Json {
    let stage = job.stage.as_deref().map_or(Vec::new(), |stage| {
        stage.split('/').map(Json::string).collect()
    });
    let inputs = job.inputs.iter().map(|(port, artifacts)| {
        (
            port.clone(),
            Json::array(artifacts.iter().map(artifact_json)),
        )
    });
    let outputs = job
        .outputs
        .iter()
        .map(|(port, artifact)| (port.clone(), artifact_json(artifact)));
    Json::object([
        ("id", Json::Number(job.id)),
        ("operation", Json::string(&job.operation)),
        ("stage", Json::Array(stage)),
        ("inputs", Json::Object(inputs.collect())),
        ("outputs", Json::Object(outputs.collect())),
        (
            "depends_on",
            Json::array(job.depends_on.iter().copied().map(Json::Number)),
        ),
        (
            "command",
            job.command.as_deref().map_or(Json::Null, command_json),
        ),
        (
            "verify",
            Json::array(job.verify.iter().map(|command| command_json(command))),
        ),
    ])
}

/// An argument is an array of parts: a string for text, `{"path": ...}` for
/// a file.
fn command_json(command: &[Argument]) -> Json {
    Json::array(command.iter().map(|argument| {
        Json::array(argument.iter().map(|part| match part {
            ArgPart::Text(text) => Json::string(text),
            ArgPart::Path(path) => Json::object([("path", Json::string(path))]),
        }))
    }))
}

fn artifact_json(artifact: &BoundArtifact) -> Json {
    let entities = artifact
        .entities
        .iter()
        .map(|(dimension, value)| (dimension.clone(), Json::string(value)));
    Json::object([
        ("product", Json::string(&artifact.product)),
        ("entities", Json::Object(entities.collect())),
        ("type", type_json(&artifact.artifact_type)),
        ("path", Json::string(&artifact.path)),
    ])
}

fn type_json(artifact_type: &TypeExpr) -> Json {
    match artifact_type {
        TypeExpr::Unknown => Json::Null,
        TypeExpr::Variable(name) => Json::object([("variable", Json::string(name))]),
        TypeExpr::Named(name) => {
            Json::object([("name", Json::string(name)), ("args", Json::array([]))])
        }
        TypeExpr::Applied { constructor, args } => Json::object([
            ("name", Json::string(constructor)),
            ("args", Json::array(args.iter().map(type_json))),
        ]),
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
