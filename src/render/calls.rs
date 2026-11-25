use std::fmt::Write as _;

use crate::json::Json;
use crate::model::{CallId, Invocation, Pipeline};

const BLOB_WIDTH: usize = 7;

pub fn render_calls(pipeline: &Pipeline) -> String {
    let files = pipeline.operation_files();
    let mut text = String::new();
    let mut open: Vec<CallId> = Vec::new();
    for step in &pipeline.invocations {
        let Some(origin) = &step.origin else { continue };
        let mut chain: Vec<CallId> = std::iter::successors(Some(origin.call), |call| {
            pipeline.calls[call.index()].parent
        })
        .collect();
        chain.reverse();
        let shared = open
            .iter()
            .zip(&chain)
            .take_while(|(left, right)| left == right)
            .count();
        for (depth, id) in chain.iter().enumerate().skip(shared) {
            let call = &pipeline.calls[id.index()];
            if depth == 0 {
                write!(
                    text,
                    "{} = {}(…)  line {}",
                    call.outputs.join(", "),
                    call.operation,
                    call.place.line
                )
                .expect("writing to a String");
                if let Some(stage) = &step.stage {
                    write!(text, "  [{stage}]").expect("writing to a String");
                }
                text.push('\n');
                let source = files
                    .get(call.operation.as_str())
                    .copied()
                    .flatten()
                    .map(|file| &pipeline.files[file]);
                write!(text, "  {}", call.operation).expect("writing to a String");
                if let Some(file) = source {
                    let blob: String = file.blob.chars().take(BLOB_WIDTH).collect();
                    write!(text, "  {}  blob {blob}", file.path).expect("writing to a String");
                }
                text.push('\n');
            } else {
                let inputs = call.inputs.join(", ");
                writeln!(
                    text,
                    "{}line {}  {} = {}({inputs})",
                    "  ".repeat(depth),
                    call.place.line,
                    call.outputs.join(", "),
                    call.operation,
                )
                .expect("writing to a String");
            }
        }
        writeln!(
            text,
            "{}line {}  {}",
            "  ".repeat(chain.len()),
            origin.step.line,
            written_step(step)
        )
        .expect("writing to a String");
        open = chain;
    }
    text
}

pub(crate) fn written_step(step: &Invocation) -> String {
    let inputs: Vec<&str> = step
        .inputs
        .iter()
        .map(|input| input.product_name())
        .collect();
    format!(
        "{} = {}({})",
        step.outputs.join(", "),
        step.operation,
        inputs.join(", ")
    )
}

pub fn render_calls_json(pipeline: &Pipeline) -> String {
    let files = pipeline.operation_files();
    let mut steps: Vec<Vec<&Invocation>> = vec![Vec::new(); pipeline.calls.len()];
    let mut stages: Vec<Option<&str>> = vec![None; pipeline.calls.len()];
    for step in &pipeline.invocations {
        let Some(origin) = &step.origin else { continue };
        steps[origin.call.index()].push(step);
        let mut call = Some(origin.call);
        while let Some(id) = call {
            stages[id.index()].get_or_insert(step.stage.as_deref().unwrap_or(""));
            call = pipeline.calls[id.index()].parent;
        }
    }
    let optional =
        |value: Option<&str>| value.map_or(Json::Null, |text| Json::string(text.to_owned()));
    let calls = pipeline
        .calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| {
            let stage = (*stages.get(index)?)?;
            let file = files
                .get(call.operation.as_str())
                .copied()
                .flatten()
                .map(|file| &pipeline.files[file]);
            let step_objects = steps[index].iter().filter_map(|step| {
                let origin = step.origin.as_ref()?;
                Some(Json::object([
                    ("line", Json::Number(origin.step.line)),
                    ("operation", Json::string(&step.operation)),
                    ("outputs", strings(&step.outputs)),
                    (
                        "inputs",
                        Json::array(
                            step.inputs
                                .iter()
                                .map(|input| Json::string(input.product_name())),
                        ),
                    ),
                ]))
            });
            Some(Json::object([
                ("id", Json::Number(index)),
                (
                    "parent",
                    Json::number_or_null(call.parent.map(CallId::index)),
                ),
                ("operation", Json::string(&call.operation)),
                ("outputs", strings(&call.outputs)),
                ("inputs", strings(&call.inputs)),
                ("line", Json::Number(call.place.line)),
                ("stage", optional((!stage.is_empty()).then_some(stage))),
                ("file", optional(file.map(|file| file.path.as_str()))),
                ("blob", optional(file.map(|file| file.blob.as_str()))),
                ("steps", Json::array(step_objects)),
            ]))
        });
    Json::array(calls).to_string()
}

fn strings(items: &[String]) -> Json<'_> {
    Json::array(items.iter().map(|item| Json::string(item.as_str())))
}
