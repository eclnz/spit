//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::json::{write_array, write_number, write_string, ObjectWriter, Out};
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
    /// Each input port and its artifacts, in port order. Jobs that use the
    /// same artifact share it.
    pub inputs: Vec<(String, Vec<Arc<BoundArtifact>>)>,
    /// Each output port and its artifact, in port order.
    pub outputs: Vec<(String, Arc<BoundArtifact>)>,
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
        self.inputs
            .iter()
            .flat_map(|(_, artifacts)| artifacts)
            .map(Arc::as_ref)
    }
}

impl BoundDag {
    /// The inputs no job here makes, each once, by path: sources, and the
    /// outputs of stages left out.
    pub fn external_inputs(&self) -> Vec<&BoundArtifact> {
        let produced: FxHashSet<_> = self
            .jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .map(|(_, artifact)| artifact.path.as_str())
            .collect();
        let mut external = FxHashMap::default();
        for artifact in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            if !produced.contains(artifact.path.as_str()) {
                external.entry(artifact.path.as_str()).or_insert(artifact);
            }
        }
        let mut external: Vec<_> = external.into_iter().collect();
        external.sort_unstable_by_key(|&(path, _)| path);
        external.into_iter().map(|(_, artifact)| artifact).collect()
    }

    /// The outputs no job here reads: what a full run leaves behind, in job
    /// order.
    pub fn targets(&self) -> Vec<&BoundArtifact> {
        let read: FxHashSet<_> = self
            .jobs
            .iter()
            .flat_map(BoundJob::input_artifacts)
            .map(|artifact| artifact.path.as_str())
            .collect();
        self.jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .map(|(_, artifact)| artifact.as_ref())
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
        let mut out = String::new();
        // The text is kept whole, so there is nothing to hand on.
        let kept: io::Result<()> = write_document(self, &mut out, |_| Ok(()));
        debug_assert!(kept.is_ok());
        out
    }

    /// Write the `.spitdag` document to `writer` a piece at a time, without
    /// holding it all.
    pub fn write_json(&self, writer: &mut impl io::Write) -> io::Result<()> {
        let mut out = String::with_capacity(2 * PIECE);
        write_document(self, &mut out, |out| {
            writer.write_all(out.as_bytes())?;
            out.clear();
            Ok(())
        })?;
        writer.write_all(out.as_bytes())?;
        writer.flush()
    }
}

/// How much of a document [`BoundDag::write_json`] holds before writing it.
const PIECE: usize = 1 << 16;

/// A bound DAG as a `.spitdag`, ending in a newline, written a job at a
/// time into `out`. Whenever `out` holds a piece, `hand_on` may take what it
/// holds; the first error it returns stops the writing.
fn write_document(
    dag: &BoundDag,
    out: &mut String,
    mut hand_on: impl FnMut(&mut String) -> io::Result<()>,
) -> io::Result<()> {
    let mut handed = Ok(());
    let dependents = dag.dependents();
    let mut document = ObjectWriter::start(out);
    document.field("version", |out| write_number(out, SPITDAG_VERSION));
    document.field("generator", |out| {
        let mut generator = ObjectWriter::start(out);
        generator.string("name", "spit");
        generator.string("version", env!("CARGO_PKG_VERSION"));
        generator.finish();
    });
    document.field("root", |out| match &dag.root {
        Some(root) => write_string(out, root),
        None => out.push_str("null"),
    });
    document.field("external_inputs", |out| {
        write_array(out, dag.external_inputs(), write_artifact);
    });
    document.field("targets", |out| {
        write_array(out, dag.targets(), write_artifact);
    });
    document.field("executables", |out| {
        write_array(out, dag.executables(), |out, name| write_string(out, &name));
    });
    let mut work = String::new();
    document.field("jobs", |out| {
        write_array(out, &dag.jobs, |out, job| {
            if handed.is_err() {
                return;
            }
            let dependents = dependents.get(&job.id).map_or(&[][..], Vec::as_slice);
            write_job(out, job, dependents, &mut work);
            if out.len() >= PIECE {
                handed = hand_on(out);
            }
        });
    });
    document.finish();
    out.push('\n');
    handed
}

/// One job. What it reads, writes and runs is written first into `work`, a
/// buffer reused between jobs, as the object its fingerprint hashes; the
/// job's own fields then copy from it.
fn write_job(out: &mut String, job: &BoundJob, dependents: &[usize], work: &mut String) {
    work.clear();
    let mut object = ObjectWriter::start(work);
    let operation = object.field_at("operation", |out| write_string(out, &job.operation));
    let inputs = object.field_at("inputs", |out| {
        let mut inputs = ObjectWriter::start(out);
        for (port, artifacts) in &job.inputs {
            inputs.field(port, |out| {
                write_array(out, artifacts, |out, artifact| {
                    write_artifact(out, artifact)
                });
            });
        }
        inputs.finish();
    });
    let outputs = object.field_at("outputs", |out| {
        let mut outputs = ObjectWriter::start(out);
        for (port, artifact) in &job.outputs {
            outputs.field(port, |out| write_artifact(out, artifact));
        }
        outputs.finish();
    });
    let command = object.field_at("command", |out| match &job.command {
        Some(command) => write_command(out, command),
        None => out.push_str("null"),
    });
    let verify = object.field_at("verify", |out| {
        write_array(out, &job.verify, |out, command| write_command(out, command));
    });
    object.finish();

    let mut object = ObjectWriter::start(out);
    object.field("id", |out| write_number(out, job.id));
    object.raw("operation", &work[operation]);
    object.field("stage", |out| {
        write_array(
            out,
            job.stage.iter().flat_map(|stage| stage.split('/')),
            write_string,
        );
    });
    object.field("fingerprint", |out| {
        out.push('"');
        write_hex(out, fingerprint(work));
        out.push('"');
    });
    object.raw("inputs", &work[inputs]);
    object.raw("outputs", &work[outputs]);
    object.field("depends_on", |out| {
        write_array(out, job.depends_on.iter().copied(), write_number);
    });
    object.field("dependents", |out| {
        write_array(out, dependents.iter().copied(), write_number);
    });
    object.raw("command", &work[command]);
    object.raw("verify", &work[verify]);
    object.finish();
}

/// A 64-bit FNV-1a hash of a job's work written as compact JSON, as 16
/// hexadecimal digits: the same on every platform and in every release,
/// unlike the standard library's. The JSON writer's format is part of the
/// `.spitdag` contract; a test pins a job's value.
fn fingerprint(work: &str) -> u64 {
    let mut hash = Fnv::new();
    hash.push_str(work);
    hash.0
}

/// `value` as 16 hexadecimal digits.
fn write_hex(out: &mut String, value: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for shift in (0..16).rev() {
        out.push(char::from(HEX[(value >> (4 * shift)) as usize & 0xf]));
    }
}

/// FNV-1a over the text written to it, so nothing is kept but the hash.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Out for Fnv {
    fn push_str(&mut self, text: &str) {
        for byte in text.bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// An argument is an array of parts: a string for text, `{"path": ...}` for
/// a file.
fn write_command(out: &mut String, command: &[Argument]) {
    write_array(out, command, |out, argument| {
        write_array(out, argument, |out, part| match part {
            ArgPart::Text(text) => write_string(out, text),
            ArgPart::Path(path) => {
                out.push_str("{\"path\":");
                write_string(out, path);
                out.push('}');
            }
        });
    });
}

// Artifacts are most of a `.spitdag`, so their fixed keys are written as
// whole pieces rather than a field at a time.

fn write_artifact(out: &mut String, artifact: &BoundArtifact) {
    out.push_str("{\"product\":");
    write_string(out, &artifact.product);
    out.push_str(",\"entities\":{");
    for (index, (dimension, value)) in artifact.entities.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(out, dimension);
        out.push(':');
        write_string(out, value);
    }
    out.push_str("},\"type\":");
    write_type(out, &artifact.artifact_type);
    out.push_str(",\"path\":");
    write_string(out, &artifact.path);
    out.push('}');
}

fn write_type(out: &mut String, artifact_type: &TypeExpr) {
    match artifact_type {
        TypeExpr::Unknown => out.push_str("null"),
        TypeExpr::Variable(name) => {
            out.push_str("{\"variable\":");
            write_string(out, name);
            out.push('}');
        }
        TypeExpr::Named(name) => {
            out.push_str("{\"name\":");
            write_string(out, name);
            out.push_str(",\"args\":[]}");
        }
        TypeExpr::Applied { constructor, args } => {
            out.push_str("{\"name\":");
            write_string(out, constructor);
            out.push_str(",\"args\":");
            write_array(out, args, write_type);
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(product: &str, path: &str) -> Arc<BoundArtifact> {
        Arc::new(BoundArtifact {
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
        })
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
        // The hash of the work as compact JSON: a change to the JSON writer
        // changes every fingerprint, so it must be deliberate.
        assert_eq!(print(&job), "72f6d8ecbacfd9ad");
        let fnv = |text: &str| {
            let mut hex = String::new();
            write_hex(&mut hex, fingerprint(text));
            hex
        };
        assert_eq!(fnv(""), "cbf29ce484222325");
        assert_eq!(fnv("a"), "af63dc4c8601ec8c");
    }
}
