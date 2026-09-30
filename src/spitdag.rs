//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::io::{self, Write};
use std::sync::Arc;

use fnv::FnvHasher;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::ser::{Error, SerializeMap, SerializeStruct, Serializer};
use serde::Serialize;

use crate::json;
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
        format!("{}\n", json::to_string(&Document(self)))
    }

    /// Write the `.spitdag` document to `writer` as it is made, without
    /// holding it all.
    pub fn write_json(&self, writer: &mut impl io::Write) -> io::Result<()> {
        let mut writer = io::BufWriter::new(writer);
        json::write(&mut writer, &Document(self))?;
        writer.write_all(b"\n")?;
        writer.flush()
    }
}

/// A bound DAG as a `.spitdag`.
struct Document<'a>(&'a BoundDag);

impl Serialize for Document<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Generator {
            name: &'static str,
            version: &'static str,
        }
        let dag = self.0;
        let dependents = dag.dependents();
        let jobs: Vec<_> = (dag.jobs.iter())
            .map(|job| Job {
                job,
                dependents: dependents.get(&job.id).map_or(&[], Vec::as_slice),
            })
            .collect();
        let mut document = serializer.serialize_struct("Document", 7)?;
        document.serialize_field("version", &SPITDAG_VERSION)?;
        let generator = Generator {
            name: "spit",
            version: env!("CARGO_PKG_VERSION"),
        };
        document.serialize_field("generator", &generator)?;
        document.serialize_field("root", &dag.root)?;
        document.serialize_field("external_inputs", &dag.external_inputs())?;
        document.serialize_field("targets", &dag.targets())?;
        document.serialize_field("executables", &dag.executables())?;
        document.serialize_field("jobs", &jobs)?;
        document.end()
    }
}

/// A job with the jobs that depend on it.
struct Job<'a> {
    job: &'a BoundJob,
    dependents: &'a [usize],
}

impl Serialize for Job<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let job = self.job;
        let work = Work::from(job);
        let stage: Vec<_> = job
            .stage
            .iter()
            .flat_map(|stage| stage.split('/'))
            .collect();
        let mut object = serializer.serialize_struct("Job", 10)?;
        object.serialize_field("id", &job.id)?;
        object.serialize_field("operation", &work.operation)?;
        object.serialize_field("stage", &stage)?;
        let fingerprint = work.fingerprint().map_err(S::Error::custom)?;
        object.serialize_field("fingerprint", &format_args!("{fingerprint:016x}"))?;
        object.serialize_field("inputs", &work.inputs)?;
        object.serialize_field("outputs", &work.outputs)?;
        object.serialize_field("depends_on", &job.depends_on)?;
        object.serialize_field("dependents", self.dependents)?;
        object.serialize_field("command", &work.command)?;
        object.serialize_field("verify", &work.verify)?;
        object.end()
    }
}

/// What a job reads, writes and runs, but not its ID, stage or neighbours,
/// which can change while the work stays the same.
#[derive(Serialize)]
struct Work<'a> {
    operation: &'a str,
    inputs: Ports<'a, Vec<Arc<BoundArtifact>>>,
    outputs: Ports<'a, Arc<BoundArtifact>>,
    command: &'a Option<Vec<Argument>>,
    verify: &'a [Vec<Argument>],
}

impl<'a> From<&'a BoundJob> for Work<'a> {
    fn from(job: &'a BoundJob) -> Self {
        Self {
            operation: &job.operation,
            inputs: Ports(&job.inputs),
            outputs: Ports(&job.outputs),
            command: &job.command,
            verify: &job.verify,
        }
    }
}

impl Work<'_> {
    /// A 64-bit FNV-1a hash of the work written as compact JSON: the same
    /// on every platform and in every release, unlike the standard
    /// library's. The JSON's format is part of the `.spitdag` contract; a
    /// test pins a job's value.
    fn fingerprint(&self) -> serde_json::Result<u64> {
        struct Hashing(FnvHasher);
        impl io::Write for Hashing {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.write(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut hash = Hashing(FnvHasher::default());
        json::write(&mut hash, self)?;
        Ok(hash.0.finish())
    }
}

/// Named values in order, as an object.
struct Ports<'a, T>(&'a [(String, T)]);

impl<T: Serialize> Serialize for Ports<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(name, value)| (name, value)))
    }
}

/// An artifact: its product, its entities in declared order, its type and
/// its path.
impl Serialize for BoundArtifact {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_struct("BoundArtifact", 4)?;
        object.serialize_field("product", &self.product)?;
        object.serialize_field("entities", &Ports(&self.entities))?;
        object.serialize_field("type", &Type(&self.artifact_type))?;
        object.serialize_field("path", &self.path)?;
        object.end()
    }
}

/// A type: `null` when unknown, `{"variable": ...}`, or its name and
/// arguments.
struct Type<'a>(&'a TypeExpr);

impl Serialize for Type<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (name, args) = match self.0 {
            TypeExpr::Unknown => return serializer.serialize_none(),
            TypeExpr::Variable(name) => {
                let mut object = serializer.serialize_map(Some(1))?;
                object.serialize_entry("variable", name)?;
                return object.end();
            }
            TypeExpr::Named(name) => (name, &[][..]),
            TypeExpr::Applied { constructor, args } => (constructor, args.as_slice()),
        };
        let mut object = serializer.serialize_map(Some(2))?;
        object.serialize_entry("name", name)?;
        object.serialize_entry("args", &args.iter().map(Type).collect::<Vec<_>>())?;
        object.end()
    }
}

/// A part of an argument: a string for text, `{"path": ...}` for a file.
impl Serialize for ArgPart {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(text) => serializer.serialize_str(text),
            Self::Path(path) => {
                let mut object = serializer.serialize_map(Some(1))?;
                object.serialize_entry("path", path)?;
                object.end()
            }
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
            let mut hash = FnvHasher::default();
            hash.write(text.as_bytes());
            format!("{:016x}", hash.finish())
        };
        assert_eq!(fnv(""), "cbf29ce484222325");
        assert_eq!(fnv("a"), "af63dc4c8601ec8c");
    }
}
