//! The bound DAG: what step 3 hands a backend. Every job carries its
//! artifacts' paths and its commands as argument lists, so a backend needs
//! nothing else; it never sees the pipeline, a path rule or a command
//! template. Written as a `.spitdag`, a JSON document.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::ops::Range;

use crate::json::{write_array, write_number, write_string, ObjectWriter, Out};
use crate::model::{identity, natural_cmp, ArtifactId, Artifacts, EntityBinding, Removal};
use crate::types::TypeExpr;

/// The schema version a `.spitdag` is written with.
pub const SPITDAG_VERSION: usize = 3;

/// A resolved DAG with its paths bound and its commands expanded. Its
/// artifacts are the resolved DAG's, each kept once with its path; jobs,
/// and the paths in their commands, refer to them by id.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BoundDag {
    /// The absolute dataset folder every path is relative to, when known.
    pub root: Option<String>,
    /// What the input stage left out of the dataset, and why.
    pub removed: Vec<Removal>,
    /// Each job after the jobs it depends on.
    pub jobs: Vec<BoundJob>,
    artifacts: Artifacts,
    /// Each artifact's file, relative to the dataset root, by id; empty for
    /// an artifact no job uses.
    paths: Vec<String>,
    /// Each product's dimensions in declared order, by product number.
    dimensions: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJob {
    pub id: usize,
    pub operation: String,
    /// The stage of the step that made this job, as `outer/inner`.
    pub stage: Option<String>,
    /// Each input port and its artifacts, in port order.
    pub inputs: Vec<(String, Vec<ArtifactId>)>,
    /// Each output port and its artifact, in port order.
    pub outputs: Vec<(String, ArtifactId)>,
    pub depends_on: Vec<usize>,
    /// The command that makes the outputs; `None` when the operation has none.
    pub command: Option<Vec<Argument>>,
    /// Commands that check the inputs before the job runs.
    pub verify: Vec<Vec<Argument>>,
}

/// An artifact of a bound DAG, with its path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundArtifact<'a> {
    pub product: &'a str,
    pub artifact_type: &'a TypeExpr,
    /// The file, relative to the dataset root.
    pub path: &'a str,
    entities: &'a EntityBinding,
    dimensions: &'a [String],
}

/// One command-line argument: literal text and artifact paths, joined.
pub type Argument = Vec<ArgPart>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgPart {
    Text(String),
    /// An artifact's file, relative to the dataset root.
    Path(ArtifactId),
}

impl<'a> BoundArtifact<'a> {
    /// Each dimension and value, in the product's declared order.
    pub fn entities(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        let entities = self.entities;
        self.dimensions
            .iter()
            .filter_map(|dimension| Some((dimension.as_str(), entities.get(dimension)?)))
    }

    /// Its product and bindings, as `image[sub=1,ses=2]`.
    pub fn identity(&self) -> String {
        identity(self.product, self.entities())
    }
}

impl BoundJob {
    /// Every input artifact, in port order.
    pub fn input_artifacts(&self) -> impl Iterator<Item = ArtifactId> + '_ {
        self.inputs
            .iter()
            .flat_map(|(_, artifacts)| artifacts)
            .copied()
    }
}

impl BoundDag {
    /// `jobs` over `artifacts`, where the artifact with each id has the path
    /// `paths` holds for it and each product, by number, has `dimensions`.
    pub(crate) fn new(
        artifacts: Artifacts,
        paths: Vec<String>,
        dimensions: Vec<Vec<String>>,
        jobs: Vec<BoundJob>,
    ) -> Self {
        Self {
            root: None,
            removed: Vec::new(),
            jobs,
            artifacts,
            paths,
            dimensions,
        }
    }

    pub fn artifact(&self, id: ArtifactId) -> BoundArtifact<'_> {
        let artifact = self.artifacts.get(id);
        BoundArtifact {
            product: artifact.product,
            artifact_type: artifact.artifact_type,
            path: &self.paths[id.index()],
            entities: artifact.entities,
            dimensions: &self.dimensions[self.artifacts.product_of(id) as usize],
        }
    }

    /// The path of `id`, relative to the dataset root.
    pub fn path(&self, id: ArtifactId) -> &str {
        &self.paths[id.index()]
    }

    /// Whether each artifact, by id, is an output of a job here.
    fn produced(&self) -> Vec<bool> {
        let mut produced = vec![false; self.paths.len()];
        for &(_, output) in self.jobs.iter().flat_map(|job| &job.outputs) {
            produced[output.index()] = true;
        }
        produced
    }

    /// The inputs no job here makes, each once, by path: sources, and the
    /// outputs of stages left out.
    pub fn external_inputs(&self) -> Vec<BoundArtifact<'_>> {
        let mut seen = self.produced();
        let mut external = Vec::new();
        for input in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            if !seen[input.index()] {
                seen[input.index()] = true;
                external.push(input);
            }
        }
        // In the order `many` inputs take, so `wave10` follows `wave2`. Every
        // artifact has its own path, so the order is total.
        external.sort_unstable_by(|&left, &right| natural_cmp(self.path(left), self.path(right)));
        external.into_iter().map(|id| self.artifact(id)).collect()
    }

    /// The outputs no job here reads: what a full run leaves behind, in job
    /// order.
    pub fn targets(&self) -> Vec<BoundArtifact<'_>> {
        let mut read = vec![false; self.paths.len()];
        for input in self.jobs.iter().flat_map(BoundJob::input_artifacts) {
            read[input.index()] = true;
        }
        self.jobs
            .iter()
            .flat_map(|job| &job.outputs)
            .filter(|(_, output)| !read[output.index()])
            .map(|&(_, output)| self.artifact(output))
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
    let artifacts = ArtifactJson::new(dag);
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
        write_array(out, dag.external_inputs(), |out, artifact| {
            write_artifact(out, &artifact);
        });
    });
    document.field("targets", |out| {
        write_array(out, dag.targets(), |out, artifact| {
            write_artifact(out, &artifact);
        });
    });
    document.field("executables", |out| {
        write_array(out, dag.executables(), |out, name| write_string(out, &name));
    });
    document.field("removed", |out| {
        write_array(out, &dag.removed, |out, removal| {
            write_removal(out, removal)
        });
    });
    let mut work = String::new();
    document.field("jobs", |out| {
        write_array(out, &dag.jobs, |out, job| {
            if handed.is_err() {
                return;
            }
            let dependents = dependents.get(&job.id).map_or(&[][..], Vec::as_slice);
            write_job(out, dag, &artifacts, job, dependents, &mut work);
            if out.len() >= PIECE {
                handed = hand_on(out);
            }
        });
    });
    document.finish();
    out.push('\n');
    handed
}

/// Each artifact a job uses, as JSON, written once however many jobs use it.
struct ArtifactJson {
    text: String,
    /// Where each artifact's JSON is in `text`, by id; empty for one no job
    /// uses.
    spans: Vec<Range<usize>>,
}

impl ArtifactJson {
    fn new(dag: &BoundDag) -> Self {
        let mut used = vec![false; dag.paths.len()];
        for job in &dag.jobs {
            for id in job
                .input_artifacts()
                .chain(job.outputs.iter().map(|&(_, id)| id))
            {
                used[id.index()] = true;
            }
        }
        let mut text = String::new();
        let spans = dag
            .artifacts
            .ids()
            .map(|id| {
                let start = text.len();
                if used[id.index()] {
                    write_artifact(&mut text, &dag.artifact(id));
                }
                start..text.len()
            })
            .collect();
        Self { text, spans }
    }

    fn get(&self, id: ArtifactId) -> &str {
        &self.text[self.spans[id.index()].clone()]
    }
}

/// One job. What it reads, writes and runs is written first into `work`, a
/// buffer reused between jobs, as the object its fingerprint hashes; the
/// job's own fields then copy from it.
fn write_job(
    out: &mut String,
    dag: &BoundDag,
    artifacts: &ArtifactJson,
    job: &BoundJob,
    dependents: &[usize],
    work: &mut String,
) {
    work.clear();
    let mut object = ObjectWriter::start(work);
    let operation = object.field_at("operation", |out| write_string(out, &job.operation));
    let inputs = object.field_at("inputs", |out| {
        let mut inputs = ObjectWriter::start(out);
        for (port, ids) in &job.inputs {
            inputs.field(port, |out| {
                write_array(out, ids, |out, &id| out.push_str(artifacts.get(id)));
            });
        }
        inputs.finish();
    });
    let outputs = object.field_at("outputs", |out| {
        let mut outputs = ObjectWriter::start(out);
        for (port, artifact) in &job.outputs {
            outputs.field(port, |out| out.push_str(artifacts.get(*artifact)));
        }
        outputs.finish();
    });
    let command = object.field_at("command", |out| match &job.command {
        Some(command) => write_command(out, dag, command),
        None => out.push_str("null"),
    });
    let verify = object.field_at("verify", |out| {
        write_array(out, &job.verify, |out, command| {
            write_command(out, dag, command);
        });
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
fn write_command(out: &mut String, dag: &BoundDag, command: &[Argument]) {
    write_array(out, command, |out, argument| {
        write_array(out, argument, |out, part| match part {
            ArgPart::Text(text) => write_string(out, text),
            ArgPart::Path(artifact) => {
                out.push_str("{\"path\":");
                write_string(out, dag.path(*artifact));
                out.push('}');
            }
        });
    });
}

// Artifacts are most of a `.spitdag`, so their fixed keys are written as
// whole pieces rather than a field at a time.

fn write_artifact(out: &mut String, artifact: &BoundArtifact<'_>) {
    out.push_str("{\"product\":");
    write_string(out, artifact.product);
    out.push_str(",\"entities\":{");
    for (index, (dimension, value)) in artifact.entities().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(out, dimension);
        out.push(':');
        write_string(out, value);
    }
    out.push_str("},\"type\":");
    write_type(out, artifact.artifact_type);
    out.push_str(",\"path\":");
    write_string(out, artifact.path);
    out.push('}');
}

/// A removal as `{"product": ..., "entities": {...}, "rule": ..., "origin":
/// ..., "reason": ...}`, with `null` for a group's product and for an
/// origin or reason that is not known.
fn write_removal(out: &mut String, removal: &Removal) {
    let optional = |out: &mut String, value: Option<&str>| match value {
        Some(value) => write_string(out, value),
        None => out.push_str("null"),
    };
    out.push_str("{\"product\":");
    optional(out, removal.product.as_deref());
    out.push_str(",\"entities\":{");
    for (index, (dimension, value)) in removal.entities.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(out, dimension);
        out.push(':');
        write_string(out, value);
    }
    out.push_str("},\"rule\":");
    write_string(out, &removal.rule);
    out.push_str(",\"origin\":");
    optional(out, removal.origin.as_deref());
    out.push_str(",\"reason\":");
    optional(out, removal.reason.as_deref());
    out.push_str(",\"found\":");
    match removal.found {
        Some(found) => write_number(out, found),
        None => out.push_str("null"),
    }
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

    /// A DAG of the jobs `jobs` makes from the ids of `raw`, `clean` and
    /// `mean`: one artifact each, at `in/1.txt`, `out/1.txt` and
    /// `out/mean.txt`, with entities that need escaping.
    fn bound(jobs: impl FnOnce([ArtifactId; 3]) -> Vec<BoundJob>) -> BoundDag {
        let artifact_type = TypeExpr::applied(
            "MRI",
            vec![
                TypeExpr::applied("Pair", vec![TypeExpr::named("T1w"), TypeExpr::Unknown]),
                TypeExpr::named("Diffusion"),
            ],
        );
        let entities = EntityBinding::from_pairs([("sub", "1"), ("run", "a\"\\\n\u{1}é")]);
        let mut artifacts = Artifacts::default();
        let ids = ["raw", "clean", "mean"].map(|product| {
            let number = artifacts.product(product, &artifact_type);
            artifacts.add(number, entities.clone()).unwrap()
        });
        let paths = ["in/1.txt", "out/1.txt", "out/mean.txt"].map(String::from);
        let dimensions = vec![vec!["sub".to_owned(), "run".to_owned()]; 3];
        BoundDag::new(artifacts, paths.to_vec(), dimensions, jobs(ids))
    }

    #[test]
    fn a_spitdag_is_written_with_its_version_and_escapes() {
        let mut dag = bound(|[raw, clean, mean]| {
            vec![
                BoundJob {
                    id: 1,
                    operation: "clean".into(),
                    stage: Some("prep/denoise".into()),
                    inputs: vec![("raw".into(), vec![raw])],
                    outputs: vec![("output".into(), clean)],
                    depends_on: vec![],
                    command: Some(vec![
                        vec![ArgPart::Text("tool".into())],
                        vec![ArgPart::Text("--in=".into()), ArgPart::Path(raw)],
                    ]),
                    verify: vec![vec![vec![ArgPart::Path(raw)]]],
                },
                BoundJob {
                    id: 2,
                    operation: "mean".into(),
                    stage: None,
                    inputs: vec![("frames".into(), vec![clean])],
                    outputs: vec![("output".into(), mean)],
                    depends_on: vec![1],
                    command: None,
                    verify: vec![],
                },
            ]
        });
        dag.root = Some("/data/study".into());
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
            text.contains("\"executables\":[\"tool\"],\"removed\":[],\"jobs\""),
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
    fn a_removal_is_written_with_its_rule_and_why() {
        let mut dag = bound(|_| Vec::new());
        dag.removed = vec![
            Removal {
                product: Some("bold".into()),
                entities: EntityBinding::from_pairs([("sub", "02"), ("run", "3")]),
                rule: "exclude bold[run=3,sub=02]".into(),
                origin: Some("line 4".into()),
                reason: Some("motion \"spike\"".into()),
                found: None,
            },
            Removal {
                product: None,
                entities: EntityBinding::from_pairs([("sub", "03")]),
                rule: "drop [sub] where sessions count<2".into(),
                origin: None,
                reason: None,
                found: Some(1),
            },
        ];
        let text = dag.to_json();
        assert!(
            text.contains(
                "\"removed\":[{\"product\":\"bold\",\"entities\":{\"run\":\"3\",\"sub\":\"02\"},\
\"rule\":\"exclude bold[run=3,sub=02]\",\"origin\":\"line 4\",\"reason\":\"motion \\\"spike\\\"\",\
\"found\":null},{\"product\":null,\"entities\":{\"sub\":\"03\"},\"rule\":\"drop [sub] where sessions count<2\",\
\"origin\":null,\"reason\":null,\"found\":1}]"
            ),
            "{text}"
        );
    }

    #[test]
    fn external_inputs_are_in_the_order_many_inputs_take() {
        let artifact_type = TypeExpr::named("Table");
        let mut artifacts = Artifacts::default();
        let wave = artifacts.product("wave", &artifact_type);
        let fit = artifacts.product("fit", &artifact_type);
        let waves = ["1", "10", "2"].map(|value| {
            artifacts
                .add(wave, EntityBinding::from_pairs([("wave", value)]))
                .unwrap()
        });
        let output = artifacts.add(fit, EntityBinding::from_pairs([])).unwrap();
        let paths = ["w/wave1.csv", "w/wave10.csv", "w/wave2.csv", "fit.json"].map(String::from);
        let job = BoundJob {
            id: 1,
            operation: "fit".into(),
            stage: None,
            inputs: vec![("waves".into(), waves.to_vec())],
            outputs: vec![("output".into(), output)],
            depends_on: vec![],
            command: None,
            verify: vec![],
        };
        let dimensions = vec![vec!["wave".to_owned()], vec![]];
        let dag = BoundDag::new(artifacts, paths.to_vec(), dimensions, vec![job]);
        let order: Vec<_> = dag
            .external_inputs()
            .iter()
            .map(|input| input.path)
            .collect();
        assert_eq!(order, ["w/wave1.csv", "w/wave2.csv", "w/wave10.csv"]);
    }

    #[test]
    fn a_fingerprint_follows_the_work_not_the_job_number() {
        let job = |[raw, clean, _]: [ArtifactId; 3]| BoundJob {
            id: 1,
            operation: "clean".into(),
            stage: None,
            inputs: vec![("raw".into(), vec![raw])],
            outputs: vec![("output".into(), clean)],
            depends_on: vec![],
            command: Some(vec![vec![ArgPart::Text("tool".into())]]),
            verify: vec![],
        };
        let print = |make: &dyn Fn([ArtifactId; 3]) -> BoundJob| {
            let text = bound(|ids| vec![make(ids)]).to_json();
            let start = text.find("\"fingerprint\":\"").unwrap() + 15;
            text[start..start + 16].to_owned()
        };
        let renumbered = |ids| BoundJob {
            id: 7,
            stage: Some("prep".into()),
            ..job(ids)
        };
        assert_eq!(print(&job), print(&renumbered));
        let changed = |ids| BoundJob {
            command: Some(vec![vec![ArgPart::Text("other".into())]]),
            ..job(ids)
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
