//! Writing a bound DAG as a `.spitdag`: a job at a time, each artifact's
//! JSON written once.

use std::io;

use crate::json::{write_array, write_number, write_string, write_u64, ObjectWriter};
use crate::model::{ArtifactId, JobId, Removal};

use super::artifact_json::{write_artifact, ArtifactJson};
use super::work::{write_work_commands, WorkCommands};
use super::{ArgPart, Argument, BoundDag, BoundJob, BoundStep, When, SPITDAG_VERSION};

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
    document.field("output_root", |out| match &dag.output_root {
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
    document.field("pipeline_folder", |out| match &dag.pipeline_folder {
        Some(folder) => write_string(out, folder),
        None => out.push_str("null"),
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

/// One job. Its operation, commands and `verify` are written first into
/// `work`, a buffer reused between jobs, and the job's own fields copy from
/// it.
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
    let commands = write_work_commands(&mut object, dag, &job.commands);
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
    object.field("origin", |out| match &step.origin {
        Some(origin) => {
            let mut item = ObjectWriter::start(out);
            item.field("call", |out| write_optional_number(out, origin.call));
            item.field("file", |out| write_optional_number(out, origin.file));
            item.field("line", |out| write_number(out, origin.line));
            item.finish();
        }
        None => out.push_str("null"),
    });
    object.field("inputs", |out| {
        write_inputs(out, step, job, |out, id| out.push_str(artifacts.get(id)));
    });
    object.field("outputs", |out| {
        write_outputs(out, step, job, |out, id| out.push_str(artifacts.get(id)));
    });
    if !step.seeds.is_empty() {
        object.field("seed_folders", |out| {
            let mut seeds = ObjectWriter::start(out);
            for &(output, input) in &step.seeds {
                seeds.string(&step.outputs[output], &step.inputs[input]);
            }
            seeds.finish();
        });
    }
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
    object.field("commands", |out| match &commands {
        WorkCommands::None => out.push_str("[]"),
        WorkCommands::One {
            command,
            stdin,
            stdout,
        } => {
            out.push_str("[{\"command\":");
            out.push_str(&work[command.clone()]);
            for (name, span) in [(",\"stdin\":", stdin), (",\"stdout\":", stdout)] {
                out.push_str(name);
                match span {
                    Some(span) => out.push_str(&work[span.clone()]),
                    None => out.push_str("null"),
                }
            }
            out.push_str("}]");
        }
        WorkCommands::Several(span) => out.push_str(&work[span.clone()]),
    });
    object.raw("verify", &work[verify]);
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
    // `{@cores}` in a command changes the command, which a backend that
    // identifies work holds.
    object.field("cores", |out| write_u64(out, step.resources.cores.into()));
    object.field("memory", |out| match step.resources.memory {
        Some(bytes) => write_u64(out, bytes),
        None => out.push_str("null"),
    });
    object.finish();
}

/// A job's inputs as `{port: [artifact, ...]}`, each artifact written by
/// `artifact`.
fn write_inputs(
    out: &mut String,
    step: &BoundStep,
    job: &BoundJob,
    artifact: impl Fn(&mut String, ArtifactId),
) {
    let mut inputs = ObjectWriter::start(out);
    for (port, ids) in step.inputs.iter().zip(&job.inputs) {
        inputs.field(port, |out| {
            write_array(out, ids, |out, &id| artifact(out, id))
        });
    }
    inputs.finish();
}

/// A job's outputs as `{port: artifact}`, each artifact written by `artifact`.
fn write_outputs(
    out: &mut String,
    step: &BoundStep,
    job: &BoundJob,
    artifact: impl Fn(&mut String, ArtifactId),
) {
    let mut outputs = ObjectWriter::start(out);
    for (port, &id) in step.outputs.iter().zip(&job.outputs) {
        outputs.field(port, |out| artifact(out, id));
    }
    outputs.finish();
}

/// An argument is an array of parts: a string for text, `{"path": ...}` for
/// a file, and `{"dir": ..., "of": ...}` or `{"stem": ..., "of": ...}` for
/// its folder or its name without its extension, with the file they are of,
/// and `{"here": ...}` for a folder under the pipeline's.
pub(super) fn write_command(out: &mut String, dag: &BoundDag, command: &[Argument]) {
    write_array(out, command, |out, argument| {
        write_array(out, argument, |out, part| match part {
            ArgPart::Text(text) => write_string(out, text),
            ArgPart::Path(artifact) => {
                out.push_str("{\"path\":");
                write_string(out, dag.path(*artifact));
                out.push('}');
            }
            ArgPart::Here { relative, .. } => {
                out.push_str("{\"here\":");
                write_string(out, relative);
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
    use crate::model::{Artifacts, EntityBinding, StepId, Under};
    use crate::spitdag::{BoundCommand, BoundStep};
    use crate::types::TypeExpr;

    /// A command that runs `program` alone.
    fn command(program: &str) -> BoundCommand {
        BoundCommand {
            arguments: vec![vec![ArgPart::Text(program.into())]],
            stdin: None,
            stdout: None,
        }
    }

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
            seeds: vec![],
            origin: None,
            resources: crate::model::Resources::default(),
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
        bound_of_type(&artifact_type, steps, jobs)
    }

    /// [`bound`], with every artifact of type `artifact_type`.
    fn bound_of_type(
        artifact_type: &TypeExpr,
        steps: Vec<BoundStep>,
        jobs: impl FnOnce([ArtifactId; 3]) -> Vec<BoundJob>,
    ) -> BoundDag {
        let entities = EntityBinding::from_pairs([("sub", "1"), ("run", "a\"\\\n\u{1}é")]);
        let mut artifacts = Artifacts::default();
        let ids = ["raw", "clean", "mean"].map(|product| {
            let number = artifacts.product(product, artifact_type);
            artifacts.add(number, entities.clone()).unwrap()
        });
        let paths = ["in/1.txt", "out/1.txt", "out/mean.txt"].map(String::from);
        let dimensions = vec![vec!["sub".to_owned(), "run".to_owned()]; 3];
        let under = vec![Under::Root, Under::OutputRoot, Under::OutputRoot];
        BoundDag::new(
            artifacts,
            paths.to_vec(),
            under,
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
                    commands: vec![BoundCommand {
                        arguments: vec![
                            vec![ArgPart::Text("tool".into())],
                            vec![ArgPart::Text("--in=".into()), ArgPart::Path(raw)],
                        ],
                        stdin: None,
                        stdout: None,
                    }],
                    verify: vec![vec![vec![ArgPart::Path(raw)]]],
                    checks: vec![],
                },
                BoundJob {
                    id: JobId::new(2),
                    step: StepId::new(1),
                    inputs: vec![vec![clean]],
                    outputs: vec![mean],
                    depends_on: vec![JobId::new(1)],
                    commands: vec![],
                    verify: vec![],
                    checks: vec![],
                },
            ]
        });
        dag.root = Some("/data/study".into());
        dag.output_root = Some("/data/results".into());
        let text = dag.to_json();
        assert!(text.starts_with(&format!(
            "{{\"version\":13,\"generator\":{{\"name\":\"spit\",\"version\":\"{}\"}},\
\"root\":\"/data/study\",\"output_root\":\"/data/results\",\"external_inputs\":[{{\"product\":\"raw\"",
            env!("CARGO_PKG_VERSION")
        )));
        assert!(
            text.contains("\"entities\":{\"sub\":\"1\",\"run\":\"a\\\"\\\\\\n\\u0001é\"}"),
            "{text}"
        );
        assert!(
            text.contains("\"dependents\":[],\"commands\":[],\"verify\":[]"),
            "{text}"
        );
        // Each artifact names the root its path is under.
        assert!(
            text.contains("\"path\":\"in/1.txt\",\"under\":\"root\",\"kind\":\"file\"}"),
            "{text}"
        );
        assert!(
            text.contains("\"path\":\"out/1.txt\",\"under\":\"output_root\",\"kind\":\"file\"}"),
            "{text}"
        );
        // A verify command that starts with a path names no program.
        assert!(
            text.contains("\"executables\":[\"tool\"],\"removed\":[],\"left_out\":[],\"pipeline_folder\":null,\"pipeline_files\":[],\"calls\":[],\"jobs\""),
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
    fn a_job_writes_one_or_several_commands_with_their_streams() {
        let steps = vec![step("clean", None, &["raw"], &["output"])];
        let job = |commands: Vec<BoundCommand>| {
            move |[raw, clean, _]: [ArtifactId; 3]| {
                vec![BoundJob {
                    id: JobId::new(1),
                    step: StepId::new(0),
                    inputs: vec![vec![raw]],
                    outputs: vec![clean],
                    depends_on: vec![],
                    commands,
                    verify: vec![],
                    checks: vec![],
                }]
            }
        };
        // A single command with a stdout.
        let one = bound(steps.clone(), |ids| {
            job(vec![BoundCommand {
                stdout: Some(ids[1]),
                ..command("tool")
            }])(ids)
        });
        let text = one.to_json();
        assert!(
            text.contains(
                "\"commands\":[{\"command\":[[\"tool\"]],\"stdin\":null,\"stdout\":\"out/1.txt\"}]"
            ),
            "{text}"
        );
        // Several commands are written as a list.
        let two = bound(steps, job(vec![command("tool"), command("index")])).to_json();
        assert!(
            two.contains("\"commands\":[{\"command\":[[\"tool\"]],\"stdin\":null,\"stdout\":null},{\"command\":[[\"index\"]],\"stdin\":null,\"stdout\":null}],\"verify\":[]"),
            "{two}"
        );
        assert!(
            two.contains("\"executables\":[\"index\",\"tool\"]"),
            "{two}"
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
            commands: vec![],
            verify: vec![],
            checks: vec![],
        };
        let dimensions = vec![vec!["wave".to_owned()], vec![]];
        let steps = vec![step("fit", None, &["waves"], &["output"])];
        let under = vec![Under::Root, Under::Root, Under::Root, Under::OutputRoot];
        let dag = BoundDag::new(
            artifacts,
            paths.to_vec(),
            under,
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
}
