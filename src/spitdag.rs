//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

use std::collections::{BTreeMap, BTreeSet};

use crate::json::Json;
use crate::types::TypeExpr;

/// The schema version a `.spitdag` is written with.
pub const SPITDAG_VERSION: usize = 3;

/// A resolved DAG with its paths bound and its commands expanded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundDag {
    /// The absolute dataset folder every path is relative to, when known.
    pub root: Option<String>,
    /// Each job after the jobs it depends on.
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
        let entities = self
            .entities
            .iter()
            .map(|(dimension, value)| (dimension.as_str(), value.as_str()));
        crate::model::identity(&self.product, entities)
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

    /// The outputs no job here reads: what a full run leaves behind, in job
    /// order.
    pub fn targets(&self) -> Vec<&BoundArtifact> {
        let read: BTreeSet<_> = self
            .jobs
            .iter()
            .flat_map(BoundJob::input_artifacts)
            .map(|artifact| artifact.path.as_str())
            .collect();
        self.jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .map(|(_, artifact)| artifact)
            .filter(|artifact| !read.contains(artifact.path.as_str()))
            .collect()
    }

    /// The programs the commands run, each once: every command's first
    /// argument that is plain text, so a backend can look for them before it
    /// starts.
    pub fn executables(&self) -> BTreeSet<String> {
        self.jobs
            .iter()
            .flat_map(|job| job.command.iter().chain(&job.verify))
            .filter_map(|command| {
                command
                    .first()?
                    .iter()
                    .map(|part| match part {
                        ArgPart::Text(text) => Some(text.as_str()),
                        ArgPart::Path(_) => None,
                    })
                    .collect()
            })
            .collect()
    }

    /// The jobs that depend on each job, by ID.
    pub fn dependents(&self) -> BTreeMap<usize, Vec<usize>> {
        let mut dependents: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for job in &self.jobs {
            for &dependency in &job.depends_on {
                dependents.entry(dependency).or_default().push(job.id);
            }
        }
        dependents
    }

    /// The `.spitdag` document.
    pub fn to_json(&self) -> String {
        let dependents = self.dependents();
        let document =
            Json::object([
                ("version", Json::Number(SPITDAG_VERSION)),
                (
                    "generator",
                    Json::object([
                        ("name", Json::string("spit")),
                        ("version", Json::string(env!("CARGO_PKG_VERSION"))),
                    ]),
                ),
                (
                    "root",
                    self.root.as_deref().map_or(Json::Null, Json::string),
                ),
                (
                    "external_inputs",
                    Json::array(self.external_inputs().into_iter().map(artifact_json)),
                ),
                (
                    "targets",
                    Json::array(self.targets().into_iter().map(artifact_json)),
                ),
                (
                    "executables",
                    Json::array(self.executables().into_iter().map(Json::String)),
                ),
                (
                    "jobs",
                    Json::array(self.jobs.iter().map(|job| {
                        job_json(job, dependents.get(&job.id).map_or(&[], Vec::as_slice))
                    })),
                ),
            ]);
        format!("{document}\n")
    }
}

fn job_json(job: &BoundJob, dependents: &[usize]) -> Json {
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
    let inputs = Json::Object(inputs.collect());
    let outputs = Json::Object(outputs.collect());
    let command = job.command.as_deref().map_or(Json::Null, command_json);
    let verify = Json::array(job.verify.iter().map(|command| command_json(command)));
    // What the job reads, writes and runs, but not its ID, stage or
    // neighbours, which can change while the work stays the same.
    let work = Json::object([
        ("operation", Json::string(&job.operation)),
        ("inputs", inputs.clone()),
        ("outputs", outputs.clone()),
        ("command", command.clone()),
        ("verify", verify.clone()),
    ]);
    Json::object([
        ("id", Json::Number(job.id)),
        ("operation", Json::string(&job.operation)),
        ("stage", Json::Array(stage)),
        ("fingerprint", Json::String(fingerprint(&work.to_string()))),
        ("inputs", inputs),
        ("outputs", outputs),
        (
            "depends_on",
            Json::array(job.depends_on.iter().copied().map(Json::Number)),
        ),
        (
            "dependents",
            Json::array(dependents.iter().copied().map(Json::Number)),
        ),
        ("command", command),
        ("verify", verify),
    ])
}

/// A 64-bit FNV-1a hash of `text`, as 16 hexadecimal digits: the same on
/// every platform and in every release, unlike the standard library's.
fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
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
            root: Some("/data/study".into()),
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
        assert!(text.starts_with(&format!(
            "{{\"version\":3,\"generator\":{{\"name\":\"spit\",\"version\":\"{}\"}},\
\"root\":\"/data/study\",\"external_inputs\":[{{\"product\":\"raw\"",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(
            text.contains("\"entities\":{\"sub\":\"1\",\"run\":\"a\\\"\\\\\\n\\u0001é\"}"),
            "{text}"
        );
        assert!(text.contains("\"command\":null"), "{text}");
        // A verify command that starts with a path names no program.
        assert!(
            text.contains("\"executables\":[\"tool\"],\"jobs\""),
            "{text}"
        );
        assert!(
            text.contains("\"targets\":[{\"product\":\"mean\""),
            "{text}"
        );
        assert_eq!(dag.targets().len(), 1);
        assert!(
            text.contains("\"depends_on\":[],\"dependents\":[2]"),
            "{text}"
        );
        assert!(
            text.contains("\"depends_on\":[1],\"dependents\":[]"),
            "{text}"
        );
    }

    #[test]
    fn a_fingerprint_follows_the_work_not_the_job_number() {
        let job = BoundJob {
            id: 1,
            operation: "clean".into(),
            stage: None,
            inputs: vec![("raw".into(), vec![artifact("raw", "in/1.txt")])],
            outputs: vec![("output".into(), artifact("clean", "out/1.txt"))],
            depends_on: vec![],
            command: Some(vec![vec![ArgPart::Text("tool".into())]]),
            verify: vec![],
        };
        let print = |job: &BoundJob| {
            let text = BoundDag {
                root: None,
                jobs: vec![job.clone()],
            }
            .to_json();
            let start = text.find("\"fingerprint\":\"").unwrap() + 15;
            text[start..start + 16].to_owned()
        };
        let renumbered = BoundJob {
            id: 7,
            stage: Some("prep".into()),
            ..job.clone()
        };
        assert_eq!(print(&job), print(&renumbered));
        let changed = BoundJob {
            command: Some(vec![vec![ArgPart::Text("other".into())]]),
            ..job.clone()
        };
        assert_ne!(print(&job), print(&changed));
        assert!(print(&job).bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(fingerprint(""), "cbf29ce484222325");
        assert_eq!(fingerprint("a"), "af63dc4c8601ec8c");
    }
}
