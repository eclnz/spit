//! The calls a pipeline makes to operations carried out by steps, and the
//! steps each expands to, as text. Reads the pipeline alone, so it needs
//! no data.

use std::fmt::Write as _;

use crate::json::Json;
use crate::model::{CallId, Invocation, Pipeline};

/// How many characters of a file's git blob id the listing shows.
const BLOB_WIDTH: usize = 7;

/// Each call written in the pipeline to an operation carried out by steps,
/// with the steps it expands to, in the order the steps run. A step that is
/// itself a call to such an operation is shown with its own steps under it.
///
/// ```text
/// corrected_dwi, session_b0 = mrx::clean_dwi_session(…)  line 21  [preprocess]
///   mrx::clean_dwi_session  mrtrix_dwi.spit  blob 3b18e5c
///   line 38  corrected_dwi::imported = mrx::import_dwi(raw_dwi, dwi_bvec)
/// ```
///
/// A step's line is where its operation's body writes it. Empty when the
/// pipeline makes no such call.
pub fn render_calls(pipeline: &Pipeline) -> String {
    let files = pipeline.operation_files();
    let mut text = String::new();
    let mut open: Vec<CallId> = Vec::new();
    for step in &pipeline.invocations {
        let Some(origin) = &step.origin else { continue };
        // The calls this step is nested in, outermost first.
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

/// A step as written: its outputs, its operation and the products it reads.
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

/// The calls as a JSON array, for other tools: one object per call with
/// steps, in the order of [`Pipeline::calls`], so a call's `parent` is the
/// `id` of an earlier entry.
///
/// ```text
/// {"id":0,"parent":null,"operation":"L::summarise","outputs":["m","t"],
///  "inputs":["raw","cal"],"line":6,"stage":"report","file":"libs/lib.spit",
///  "blob":"3b18e5c…","steps":[{"line":13,"operation":"L::tidy",
///  "outputs":["m::cleaned"],"inputs":["raw","cal"]}]}
/// ```
///
/// `line` is where the call is written: in the pipeline, or in the file of
/// its parent's operation. A step's `line` is in the file of its call's
/// operation. `file` and `blob` are `null` when the pipeline was read
/// without a path. A call whose steps are all calls in turn has no step
/// of its own, and is listed with its `steps` empty.
pub fn render_calls_json(pipeline: &Pipeline) -> String {
    let files = pipeline.operation_files();
    // Each call's own steps, and the stage of the first step it holds
    // anywhere, which is the call's.
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
