//! Writing a bound DAG as a `.spitdag`: a job at a time, each artifact's
//! JSON written once.

use std::io;

use crate::json::{write_array, write_number, write_string, write_u64, ObjectWriter};
use crate::model::{ArtifactId, JobId, Removal, ToolIdentity};
use crate::template::Expression;

use super::artifact_json::{write_artifact, ArtifactJson};
use super::work::{write_work_commands, WorkCommands};
use super::{ArgPart, Argument, BoundDag, BoundJob, BoundReview, BoundStep, When, SPITDAG_VERSION};

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
            write_artifact(out, dag, &artifact);
        });
    });
    document.field("targets", |out| {
        write_array(out, dag.targets(), |out, artifact| {
            write_artifact(out, dag, &artifact);
        });
    });
    document.field("executables", |out| {
        write_array(out, dag.executables(), |out, name| write_string(out, &name));
    });
    document.field("tools", |out| {
        write_array(out, &dag.tools, |out, tool| {
            let mut item = ObjectWriter::start(out);
            item.string("name", &tool.name);
            item.field("identity", |out| match &tool.identity {
                ToolIdentity::Declared(text) => write_string(out, text),
                ToolIdentity::Probe(_) => out.push_str("null"),
            });
            item.field("probe", |out| match &tool.identity {
                ToolIdentity::Probe(command) => write_array(out, command, |out, argument| {
                    write_string(out, argument);
                }),
                ToolIdentity::Declared(_) => out.push_str("null"),
            });
            item.finish();
        });
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
    document.field("reviews", |out| {
        write_array(out, &dag.reviews, |out, review| {
            write_review(out, dag, review)
        });
    });
    if !dag.screen_records.is_empty() {
        document.field("screen_records", |out| {
            let mut records = ObjectWriter::start(out);
            for (product, path) in &dag.screen_records {
                records.string(product, path);
            }
            records.finish();
        });
    }
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
    // Each step's tools as JSON, written once rather than once per job.
    let tools: Vec<String> = dag
        .steps
        .iter()
        .map(|step| {
            let mut json = String::new();
            write_array(&mut json, dag.step_tools(step), write_string);
            json
        })
        .collect();
    let mut work = String::new();
    document.field("jobs", |out| {
        write_array(out, &dag.jobs, |out, job| {
            if handed.is_err() {
                return;
            }
            let dependents = dependents.get(&job.id).map_or(&[][..], Vec::as_slice);
            write_job(
                out,
                dag,
                &artifacts,
                job,
                dependents,
                &tools[job.step.index()],
                &mut work,
            );
            if out.len() >= PIECE {
                handed = hand_on(out);
            }
        });
    });
    document.finish();
    out.push('\n');
    handed
}

/// Write the file and attachments of a review.
fn write_review(out: &mut String, dag: &BoundDag, review: &BoundReview) {
    let mut item = ObjectWriter::start(out);
    item.string("name", &review.name);
    item.field("file", |out| match &review.file {
        Some(file) => write_string(out, file),
        None => out.push_str("null"),
    });
    item.field("dimensions", |out| {
        write_array(
            out,
            review.dimensions.iter().map(String::as_str),
            write_string,
        )
    });
    item.field("targets", |out| {
        write_array(out, &review.targets, |out, target| {
            let mut item = ObjectWriter::start(out);
            item.field("artifact", |out| {
                write_artifact(out, dag, &dag.artifact(target.artifact))
            });
            item.field("input", |out| match target.input {
                Some((job, port)) => {
                    let mut input = ObjectWriter::start(out);
                    input.field("job", |out| write_number(out, job.number()));
                    input.field("port", |out| write_number(out, port));
                    input.finish();
                }
                None => out.push_str("null"),
            });
            item.finish();
        });
    });
    item.finish();
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
    tools: &str,
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
    if let Some(collection) = &step.collection {
        object.field("collection", |out| {
            let mut group = ObjectWriter::start(out);
            group.field("ports", |out| {
                write_array(out, collection.ports.iter().copied(), write_number)
            });
            group.field("varied", |out| {
                write_array(
                    out,
                    collection.varied.iter().map(String::as_str),
                    write_string,
                )
            });
            group.field("minimum", |out| write_number(out, collection.minimum));
            group.finish();
        });
    }
    if !job.missing.is_empty() {
        object.field("missing", |out| {
            write_array(out, &job.missing, |out, gap| {
                let mut object = ObjectWriter::start(out);
                object.string("message", &gap.message);
                object.field("member", |out| match &gap.member {
                    Some(values) => {
                        write_array(out, values.iter().map(String::as_str), write_string)
                    }
                    None => out.push_str("null"),
                });
                object.finish();
            });
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
    let kind = |check: &&super::BoundCheck| step.checks[check.check].kind;
    use crate::model::CheckKind;
    for (field, wanted) in [("checks", CheckKind::Check), ("screens", CheckKind::Screen)] {
        if wanted == CheckKind::Screen && !job.checks.iter().any(|check| kind(&check) == wanted) {
            continue;
        }
        object.field(field, |out| {
            write_array(
                out,
                job.checks.iter().filter(|check| kind(check) == wanted),
                |out, check| {
                    let declared = &step.checks[check.check];
                    let ports = match declared.when {
                        When::Before => &step.inputs,
                        When::After => &step.outputs,
                    };
                    let mut item = ObjectWriter::start(out);
                    item.string("when", declared.when.as_str());
                    item.string(field.trim_end_matches('s'), &declared.written);
                    item.string("port", &ports[declared.port]);
                    item.string("path", dag.path(check.artifact));
                    if wanted == CheckKind::Screen {
                        item.raw(
                            "product_scope",
                            if declared.product_scope {
                                "true"
                            } else {
                                "false"
                            },
                        );
                    }
                    item.field("command", |out| write_command(out, dag, &check.command));
                    item.finish();
                },
            );
        });
    }
    // `{@cores}` in a command changes the command, which a backend that
    // identifies work holds.
    object.field("cores", |out| write_u64(out, step.resources.cores.into()));
    object.field("memory", |out| match step.resources.memory {
        Some(bytes) => write_u64(out, bytes),
        None => out.push_str("null"),
    });
    object.raw("tools", tools);
    object.raw("anchored", if step.anchored { "true" } else { "false" });
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

/// A calculation as a JSON number, an input port name or a prefix array.
pub(super) fn write_expression(out: &mut String, expression: &Expression) {
    match expression {
        Expression::Number(text) => out.push_str(text),
        Expression::Input(name) => write_string(out, name),
        Expression::Unary(operator, operand) => {
            write_application(
                out,
                operator.as_str(),
                std::slice::from_ref(operand.as_ref()),
            );
        }
        Expression::Binary(operator, operands) => {
            write_application(out, operator.as_str(), operands.as_ref());
        }
    }
}

fn write_application(out: &mut String, operator: &str, operands: &[Expression]) {
    out.push('[');
    write_string(out, operator);
    for operand in operands {
        out.push(',');
        write_expression(out, operand);
    }
    out.push(']');
}

/// An argument is an array of parts: a string for text, `{"path": ...}` for
/// a file, and `{"dir": ..., "of": ...}` or `{"stem": ..., "of": ...}` for
/// its folder or its name without its extension, with the file they are of,
/// `{"here": ...}` for a folder under the pipeline's, `{"scalar": ...,
/// "min": ..., "max": ...}` for the number in a file, with a bound left out
/// as `null`, and `{"join": [...], "separator": ...}` or `{"list": [...]}`
/// for a `many` input's files.
pub(super) fn write_command(out: &mut String, dag: &BoundDag, command: &[Argument]) {
    write_array(out, command, |out, argument| {
        write_array(out, argument, |out, part| write_part(out, dag, part));
    });
}

fn write_part(out: &mut String, dag: &BoundDag, part: &ArgPart) {
    match part {
        ArgPart::Text(text) => write_string(out, text),
        ArgPart::Calculate(expression) => {
            out.push_str("{\"calculate\":");
            write_expression(out, expression);
            out.push('}');
        }
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
        ArgPart::Scalar { artifact, min, max } => {
            out.push_str("{\"scalar\":");
            write_string(out, dag.path(*artifact));
            // A bound is a decimal in its shortest form, which is JSON.
            for (key, bound) in [(",\"min\":", min), (",\"max\":", max)] {
                out.push_str(key);
                out.push_str(bound.as_deref().unwrap_or("null"));
            }
            out.push('}');
        }
        ArgPart::Member {
            port,
            offset,
            value,
        } => {
            let mut item = ObjectWriter::start(out);
            item.field("port", |out| write_number(out, *port));
            item.field("offset", |out| write_number(out, *offset));
            item.field("member", |out| write_part(out, dag, value));
            item.finish();
        }
        ArgPart::JoinedValues {
            port,
            values,
            separator,
        } => {
            let mut item = ObjectWriter::start(out);
            item.field("port", |out| write_number(out, *port));
            item.field("join_values", |out| {
                write_array(out, values.iter().map(String::as_str), write_string)
            });
            item.string("separator", separator);
            item.finish();
        }
        ArgPart::Collection {
            port,
            artifacts,
            separator,
        } => {
            out.push_str(if separator.is_some() {
                "{\"join\":"
            } else {
                "{\"list\":"
            });
            write_array(out, artifacts, |out, &artifact| {
                write_string(out, dag.path(artifact));
            });
            if let Some(separator) = separator {
                out.push_str(",\"separator\":");
                write_string(out, separator);
            }
            out.push_str(",\"port\":");
            write_number(out, *port);
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
    }
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
mod tests;
