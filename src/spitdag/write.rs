//! Writing a bound DAG as a `.spitdag`: a job at a time, each artifact's
//! JSON written once, and each job's fingerprint hashed from its work.

use std::io;
use std::ops::Range;

use crate::json::{write_array, write_number, write_string, ObjectWriter, Out};
use crate::model::{ArtifactId, JobId, Removal};
use crate::types::TypeExpr;

use super::{ArgPart, Argument, BoundArtifact, BoundDag, BoundJob, When, SPITDAG_VERSION};

/// How much of a document [`BoundDag::write_json`] holds before writing it.
pub(crate) const PIECE: usize = 1 << 16;

/// A bound DAG as a `.spitdag`, ending in a newline, written a job at a
/// time into `out`. Whenever `out` holds a piece, `hand_on` may take what it
/// holds; the first error it returns stops the writing.
pub(crate) fn write_document(
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
    document.field("left_out", |out| {
        write_array(out, &dag.left_out, |out, left_out| {
            let mut item = ObjectWriter::start(out);
            item.string("identity", &left_out.artifact.to_string());
            item.field("reasons", |out| {
                write_array(out, &left_out.reasons, |out, reason| {
                    write_string(out, reason)
                });
            });
            item.finish();
        });
    });
    document.field("pipeline_files", |out| {
        write_array(out, &dag.pipeline_files, |out, file| {
            let mut item = ObjectWriter::start(out);
            item.string("path", &file.path);
            item.string("blob", &file.blob);
            item.finish();
        });
    });
    document.field("calls", |out| {
        write_array(out, &dag.calls, |out, call| {
            let mut item = ObjectWriter::start(out);
            item.string("operation", &call.operation);
            item.string("instance", &call.instance);
            item.field("parent", |out| write_optional_number(out, call.parent));
            item.field("file", |out| write_optional_number(out, call.file));
            item.field("at", |out| {
                let mut at = ObjectWriter::start(out);
                at.field("file", |out| write_optional_number(out, call.at_file));
                at.field("line", |out| write_number(out, call.at_line));
                at.finish();
            });
            item.finish();
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
            for id in job.input_artifacts().chain(job.outputs.iter().copied()) {
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
    dependents: &[JobId],
    work: &mut String,
) {
    let step = dag.step(job);
    work.clear();
    let mut object = ObjectWriter::start(work);
    let operation = object.field_at("operation", |out| write_string(out, &step.operation));
    let inputs = object.field_at("inputs", |out| {
        let mut inputs = ObjectWriter::start(out);
        for (port, ids) in step.inputs.iter().zip(&job.inputs) {
            inputs.field(port, |out| {
                write_array(out, ids, |out, &id| out.push_str(artifacts.get(id)));
            });
        }
        inputs.finish();
    });
    let outputs = object.field_at("outputs", |out| {
        let mut outputs = ObjectWriter::start(out);
        for (port, &artifact) in step.outputs.iter().zip(&job.outputs) {
            outputs.field(port, |out| out.push_str(artifacts.get(artifact)));
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
    object.field("id", |out| write_number(out, job.id.number()));
    object.raw("operation", &work[operation]);
    object.field("stage", |out| {
        write_array(
            out,
            step.stage.iter().flat_map(|stage| stage.split('/')),
            write_string,
        );
    });
    // Where a job comes from is not its work, so the fingerprint leaves it
    // out, as it does the stage.
    object.field("origin", |out| match &step.origin {
        Some(origin) => {
            let mut item = ObjectWriter::start(out);
            item.field("call", |out| write_number(out, origin.call));
            item.field("line", |out| write_number(out, origin.line));
            item.finish();
        }
        None => out.push_str("null"),
    });
    object.field("fingerprint", |out| {
        out.push('"');
        write_hex(out, fingerprint(work));
        out.push('"');
    });
    object.raw("inputs", &work[inputs]);
    object.raw("outputs", &work[outputs]);
    object.field("depends_on", |out| {
        write_array(
            out,
            job.depends_on.iter().map(|id| id.number()),
            write_number,
        );
    });
    object.field("dependents", |out| {
        write_array(out, dependents.iter().map(|id| id.number()), write_number);
    });
    object.raw("command", &work[command]);
    object.raw("verify", &work[verify]);
    // A check tests the files a job reads or writes, not the job's work, so
    // the fingerprint leaves the checks out: a changed check is run again on
    // the files, without rerunning the job.
    object.field("checks", |out| {
        write_array(out, &job.checks, |out, check| {
            let declared = &step.checks[check.check];
            let ports = match declared.when {
                When::Before => &step.inputs,
                When::After => &step.outputs,
            };
            let mut item = ObjectWriter::start(out);
            item.string("when", declared.when.as_str());
            item.string("check", &declared.written);
            item.string("port", &ports[declared.port]);
            item.string("path", dag.path(check.artifact));
            item.field("command", |out| write_command(out, dag, &check.command));
            item.finish();
        });
    });
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
/// a file, and `{"dir": ..., "of": ...}` or `{"stem": ..., "of": ...}` for
/// its folder or its name without its extension, with the file they are of.
fn write_command(out: &mut String, dag: &BoundDag, command: &[Argument]) {
    write_array(out, command, |out, argument| {
        write_array(out, argument, |out, part| match part {
            ArgPart::Text(text) => write_string(out, text),
            ArgPart::Path(artifact) => {
                out.push_str("{\"path\":");
                write_string(out, dag.path(*artifact));
                out.push('}');
            }
            ArgPart::Dir(artifact) | ArgPart::Stem { artifact, .. } => {
                out.push_str(if matches!(part, ArgPart::Dir(_)) {
                    "{\"dir\":"
                } else {
                    "{\"stem\":"
                });
                write_string(out, part.text(dag));
                out.push_str(",\"of\":");
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
    out.push_str(if artifact.folder {
        ",\"kind\":\"folder\"}"
    } else {
        ",\"kind\":\"file\"}"
    });
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

/// `value`, or `null` without one.
fn write_optional_number(out: &mut String, value: Option<usize>) {
    match value {
        Some(value) => write_number(out, value),
        None => out.push_str("null"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceFile;
    use crate::model::{Artifacts, EntityBinding, StepId};
    use crate::spitdag::{BoundCall, BoundStep, StepCall};

    /// A step calling `operation` in `stage`, with ports named `inputs` and
    /// `outputs`.
    fn step(operation: &str, stage: Option<&str>, inputs: &[&str], outputs: &[&str]) -> BoundStep {
        let names = |ports: &[&str]| ports.iter().map(|&port| port.to_owned()).collect();
        BoundStep {
            operation: operation.to_owned(),
            stage: stage.map(str::to_owned),
            inputs: names(inputs),
            outputs: names(outputs),
            checks: vec![],
            origin: None,
        }
    }

    /// A DAG of `steps` and the jobs `jobs` makes from the ids of `raw`, `clean` and
    /// `mean`: one artifact each, at `in/1.txt`, `out/1.txt` and
    /// `out/mean.txt`, with entities that need escaping.
    fn bound(
        steps: Vec<BoundStep>,
        jobs: impl FnOnce([ArtifactId; 3]) -> Vec<BoundJob>,
    ) -> BoundDag {
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
        BoundDag::new(
            artifacts,
            paths.to_vec(),
            dimensions,
            vec![false; 3],
            steps,
            jobs(ids),
        )
    }

    #[test]
    fn a_spitdag_is_written_with_its_version_and_escapes() {
        let steps = vec![
            step("clean", Some("prep/denoise"), &["raw"], &["output"]),
            step("mean", None, &["frames"], &["output"]),
        ];
        let mut dag = bound(steps, |[raw, clean, mean]| {
            vec![
                BoundJob {
                    id: JobId::new(1),
                    step: StepId::new(0),
                    inputs: vec![vec![raw]],
                    outputs: vec![clean],
                    depends_on: vec![],
                    command: Some(vec![
                        vec![ArgPart::Text("tool".into())],
                        vec![ArgPart::Text("--in=".into()), ArgPart::Path(raw)],
                    ]),
                    verify: vec![vec![vec![ArgPart::Path(raw)]]],
                    checks: vec![],
                },
                BoundJob {
                    id: JobId::new(2),
                    step: StepId::new(1),
                    inputs: vec![vec![clean]],
                    outputs: vec![mean],
                    depends_on: vec![JobId::new(1)],
                    command: None,
                    verify: vec![],
                    checks: vec![],
                },
            ]
        });
        dag.root = Some("/data/study".into());
        let text = dag.to_json();
        assert!(text.starts_with(&format!(
            "{{\"version\":7,\"generator\":{{\"name\":\"spit\",\"version\":\"{}\"}},\
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
            text.contains("\"executables\":[\"tool\"],\"removed\":[],\"left_out\":[],\"pipeline_files\":[],\"calls\":[],\"jobs\""),
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
        let mut dag = bound(Vec::new(), |_| Vec::new());
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
                rule: "exclude [sub] where sessions count<2".into(),
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
\"found\":null},{\"product\":null,\"entities\":{\"sub\":\"03\"},\"rule\":\"exclude [sub] where sessions count<2\",\
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
            id: JobId::new(1),
            step: StepId::new(0),
            inputs: vec![waves.to_vec()],
            outputs: vec![output],
            depends_on: vec![],
            command: None,
            verify: vec![],
            checks: vec![],
        };
        let dimensions = vec![vec!["wave".to_owned()], vec![]];
        let steps = vec![step("fit", None, &["waves"], &["output"])];
        let dag = BoundDag::new(
            artifacts,
            paths.to_vec(),
            dimensions,
            vec![false; 2],
            steps,
            vec![job],
        );
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
            id: JobId::new(1),
            step: StepId::new(0),
            inputs: vec![vec![raw]],
            outputs: vec![clean],
            depends_on: vec![],
            command: Some(vec![vec![ArgPart::Text("tool".into())]]),
            verify: vec![],
            checks: vec![],
        };
        let print = |make: &dyn Fn([ArtifactId; 3]) -> BoundJob| {
            // The same operation and ports, outside a stage and in one.
            let steps = vec![
                step("clean", None, &["raw"], &["output"]),
                step("clean", Some("prep"), &["raw"], &["output"]),
            ];
            let text = bound(steps, |ids| vec![make(ids)]).to_json();
            let start = text.find("\"fingerprint\":\"").unwrap() + 15;
            text[start..start + 16].to_owned()
        };
        let renumbered = |ids| BoundJob {
            id: JobId::new(7),
            step: StepId::new(1),
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
        assert_eq!(print(&job), "db70745bf9c6ad61");
        let fnv = |text: &str| {
            let mut hex = String::new();
            write_hex(&mut hex, fingerprint(text));
            hex
        };
        assert_eq!(fnv(""), "cbf29ce484222325");
        assert_eq!(fnv("a"), "af63dc4c8601ec8c");
    }

    #[test]
    fn files_calls_and_origins_are_written_but_not_fingerprinted() {
        let job = |[raw, clean, _]: [ArtifactId; 3]| BoundJob {
            id: JobId::new(1),
            step: StepId::new(0),
            inputs: vec![vec![raw]],
            outputs: vec![clean],
            depends_on: vec![],
            command: Some(vec![vec![ArgPart::Text("tool".into())]]),
            verify: vec![],
            checks: vec![],
        };
        let plain = bound(vec![step("clean", None, &["raw"], &["output"])], |ids| {
            vec![job(ids)]
        });
        let mut called = step("clean", None, &["raw"], &["output"]);
        called.origin = Some(StepCall { call: 0, line: 4 });
        let mut dag = bound(vec![called], |ids| vec![job(ids)]);
        dag.pipeline_files = vec![
            SourceFile {
                path: "main.spit".into(),
                blob: "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".into(),
            },
            SourceFile {
                path: "lib/prep.spit".into(),
                blob: "ce013625030ba8dba906f756967f9e9ca394464a".into(),
            },
        ];
        dag.calls = vec![BoundCall {
            operation: "P::prep".into(),
            instance: "ready".into(),
            parent: None,
            file: Some(1),
            at_file: Some(0),
            at_line: 9,
        }];
        let text = dag.to_json();
        assert!(
            text.contains(
                "\"pipeline_files\":[{\"path\":\"main.spit\",\"blob\":\"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\"},\
{\"path\":\"lib/prep.spit\",\"blob\":\"ce013625030ba8dba906f756967f9e9ca394464a\"}],\
\"calls\":[{\"operation\":\"P::prep\",\"instance\":\"ready\",\"parent\":null,\"file\":1,\
\"at\":{\"file\":0,\"line\":9}}],\"jobs\""
            ),
            "{text}"
        );
        assert!(
            text.contains("\"origin\":{\"call\":0,\"line\":4},\"fingerprint\""),
            "{text}"
        );
        let print = |text: &str| {
            let start = text.find("\"fingerprint\":\"").unwrap() + 15;
            text[start..start + 16].to_owned()
        };
        assert!(plain.to_json().contains("\"origin\":null"));
        assert_eq!(print(&plain.to_json()), print(&text));
    }
}
