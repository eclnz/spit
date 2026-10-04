//! The calls a pipeline makes to operations carried out by steps, and the
//! steps each expands to, as text. Reads the pipeline alone, so it needs
//! no data.

use std::fmt::Write as _;

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
