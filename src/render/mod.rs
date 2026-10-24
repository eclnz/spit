//! Text reports of a resolved DAG: its jobs, and what can and cannot be made.

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use crate::command::shell_word;
use crate::model::{
    identity, push_identity, Artifact, ArtifactInstance, ArtifactReport, Call, CallId,
    EntityBinding, Gap, IncompleteJob, JobId, Pipeline, ResolvedDag,
};
use crate::spitdag::{Argument, BoundDag, BoundJob, StepCall, When};
use crate::types::TypeExpr;
pub(crate) use calls::written_step;
pub use calls::{render_calls, render_calls_json};

mod calls;
mod targets;
mod topology;
mod topology_connected;
pub use topology::render_pipeline_tree;

/// The jobs as text, without ports or paths. Each job is written straight
/// into the text, with each product's dimensions and type found and
/// formatted once rather than once per artifact.
pub fn render_dag(dag: &ResolvedDag) -> String {
    let products: Vec<ProductText<'_>> = dag
        .artifacts
        .products()
        .map(|(product, artifact_type)| ProductText {
            dimensions: dag.product_dimensions.get(product).map(Vec::as_slice),
            typed: typed(String::new(), artifact_type),
        })
        .collect();
    let mut writer = JobWriter::default();
    let artifact = |writer: &mut JobWriter, id| {
        let product = &products[dag.artifacts.product_of(id) as usize];
        let artifact = dag.artifact(id);
        writer.line(None);
        match product.dimensions {
            Some(dimensions) => push_identity(
                &mut writer.text,
                artifact.product,
                dimensions.iter().filter_map(|dimension| {
                    Some((dimension.as_str(), artifact.entities.get(dimension)?))
                }),
            ),
            None => write!(writer.text, "{artifact}").expect("writing to a String"),
        }
        writer.text.push_str(&product.typed);
        writer.text.push('\n');
    };
    for job in &dag.jobs {
        let step = dag.step(job);
        writer.head(job.id, step.stage.as_deref(), &step.operation);
        for input in job.input_artifacts() {
            artifact(&mut writer, input);
        }
        writer.outputs(job.outputs.len() == 1);
        for &output in &job.outputs {
            artifact(&mut writer, output);
        }
        writer.tail(&job.dependencies);
    }
    writer.text
}

/// How many jobs each step resolves, one row per step in the order they
/// were resolved, then the total, as
///
/// ```text
/// jobs  step                  stage
///    4  sorted = sort_lines   preprocess/clean
///    0  report = summarise    analysis
///    4  total
/// ```
///
/// A step that resolves no jobs keeps its row, so an empty step shows. The
/// steps a call to an operation carried out by steps made are indented
/// under the call, and followed by the call's jobs in all:
///
/// ```text
/// jobs  step                   stage
///       m, t = summarise       report
///    4    m::cleaned = clean   report
///    2    m = merge            report
///    2    t = count            report
///    8    in this call
///    8  total
/// ```
pub fn render_step_counts(pipeline: &Pipeline, dag: &ResolvedDag) -> String {
    let mut jobs = vec![0_usize; dag.steps.len()];
    for job in &dag.jobs {
        jobs[job.step.index()] += 1;
    }
    // The calls each step is nested in, outermost first, and each call's
    // jobs in all.
    let chains: Vec<Vec<CallId>> = dag
        .steps
        .iter()
        .map(|step| {
            let mut chain: Vec<CallId> =
                std::iter::successors(step.origin.as_ref().map(|origin| origin.call), |call| {
                    pipeline.calls[call.index()].parent
                })
                .collect();
            chain.reverse();
            chain
        })
        .collect();
    let mut in_call = vec![0_usize; pipeline.calls.len()];
    for (chain, count) in chains.iter().zip(&jobs) {
        for call in chain {
            in_call[call.index()] += count;
        }
    }
    // Each row: its count, its step indented under its calls, its stage.
    let mut rows: Vec<(String, String, &str)> = vec![("jobs".into(), "step".into(), "stage")];
    let mut open: &[CallId] = &[];
    for (((step, chain), count), stage) in dag.steps.iter().zip(&chains).zip(&jobs).zip(
        dag.steps
            .iter()
            .map(|step| step.stage.as_deref().unwrap_or("")),
    ) {
        let shared = open
            .iter()
            .zip(chain)
            .take_while(|(left, right)| left == right)
            .count();
        for (depth, call) in open.iter().enumerate().skip(shared).rev() {
            rows.push((
                in_call[call.index()].to_string(),
                format!("{}in this call", "  ".repeat(depth + 1)),
                "",
            ));
        }
        for (depth, call) in chain.iter().enumerate().skip(shared) {
            let call = &pipeline.calls[call.index()];
            rows.push((
                String::new(),
                format!(
                    "{}{} = {}",
                    "  ".repeat(depth),
                    call.outputs.join(", "),
                    call.operation
                ),
                stage,
            ));
        }
        rows.push((
            count.to_string(),
            format!(
                "{}{} = {}",
                "  ".repeat(chain.len()),
                step.outputs.join(", "),
                step.operation
            ),
            stage,
        ));
        open = chain;
    }
    for (depth, call) in open.iter().enumerate().rev() {
        rows.push((
            in_call[call.index()].to_string(),
            format!("{}in this call", "  ".repeat(depth + 1)),
            "",
        ));
    }
    rows.push((dag.jobs.len().to_string(), "total".into(), ""));
    let count_width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
    let step_width = rows.iter().map(|row| row.1.len()).max().unwrap_or(0);
    let staged = dag.steps.iter().any(|step| step.stage.is_some());
    let mut text = String::new();
    for (count, step, stage) in &rows {
        let line = if staged {
            format!("{count:>count_width$}  {step:<step_width$}  {stage}")
        } else {
            format!("{count:>count_width$}  {step}")
        };
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

/// What every artifact of one product shows: its dimensions in declared
/// order, when known, and its type as ` : Type`, or nothing when unknown.
struct ProductText<'a> {
    dimensions: Option<&'a [String]>,
    typed: String,
}

/// What a text report of a bound DAG shows of each job besides its
/// operation and stage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct View {
    /// Each artifact's file.
    pub paths: bool,
    /// The job's `verify` and command lines, quoted as a shell reads them.
    pub commands: bool,
}

/// The jobs of a bound DAG as text. With `view.paths`, each job shows its
/// artifacts with their ports and files, then its command lines when
/// `view.commands` is also set. With `view.commands` alone, each job shows
/// only its heading and command lines. Otherwise each job shows its
/// artifacts with their ports.
pub fn render_bound_dag(dag: &BoundDag, view: View) -> String {
    if view.commands && !view.paths {
        return render_commands(dag);
    }
    let mut writer = JobWriter::default();
    let artifact = |writer: &mut JobWriter, port: Option<&str>, id| {
        let artifact = dag.artifact(id);
        writer.line(port);
        writer.artifact(&artifact.identity(), artifact.artifact_type);
        if view.paths {
            writer.path(artifact.path, artifact.folder);
        }
    };
    for job in &dag.jobs {
        let step = dag.step(job);
        writer.head(job.id, step.stage.as_deref(), &step.operation);
        for (port, artifacts) in step.inputs.iter().zip(&job.inputs) {
            for &input in artifacts {
                artifact(&mut writer, Some(port), input);
            }
        }
        let single = job.outputs.len() == 1;
        writer.outputs(single);
        for (port, &output) in step.outputs.iter().zip(&job.outputs) {
            artifact(&mut writer, (!single).then_some(port.as_str()), output);
        }
        writer.tail(&job.depends_on);
        if view.commands {
            push_command_lines(&mut writer.text, dag, job);
        }
    }
    writer.text
}

/// Each job as its heading, then its commands, as its `verify` lines and its
/// command line:
///
/// ```text
/// Job 12  fit_panel  [model]
///   verify: validate_panel build/clean/ne/wave1.csv
///   run:    fit_panel --coef build/coef/ne.json build/clean/ne/wave1.csv
/// ```
fn render_commands(dag: &BoundDag) -> String {
    let mut text = String::new();
    for job in &dag.jobs {
        if !text.is_empty() {
            text.push('\n');
        }
        let step = dag.step(job);
        write!(text, "Job {}  {}", job.id, step.operation).expect("writing to a String");
        if let Some(stage) = &step.stage {
            write!(text, "  [{stage}]").expect("writing to a String");
        }
        text.push('\n');
        push_command_lines(&mut text, dag, job);
    }
    text
}

/// A job's commands in the order a backend runs them: its input checks,
/// `verify` lines, `run` line, then its output checks. A job a call made
/// first says where it comes from.
fn push_command_lines(text: &mut String, dag: &BoundDag, job: &BoundJob) {
    let step = dag.step(job);
    if let Some(origin) = &step.origin {
        text.push_str("  from:   ");
        push_origin(text, dag, origin);
        text.push('\n');
    }
    let checks = |text: &mut String, when: When| {
        for check in job
            .checks
            .iter()
            .filter(|c| step.checks[c.check].when == when)
        {
            text.push_str("  check:  ");
            push_command(text, dag, &check.command);
            text.push('\n');
        }
    };
    checks(text, When::Before);
    for verify in &job.verify {
        text.push_str("  verify: ");
        push_command(text, dag, verify);
        text.push('\n');
    }
    text.push_str("  run:    ");
    match &job.command {
        Some(command) => push_command(text, dag, command),
        None => text.push_str("(no command)"),
    }
    text.push('\n');
    checks(text, When::After);
}

/// Each call a step is nested in, outermost first, then the body's line
/// that made it:
///
/// ```text
/// m = L::summarise (main.spit line 8), m::cleaned = L::tidy (libs/lib.spit line 13), libs/lib.spit line 10
/// ```
fn push_origin(text: &mut String, dag: &BoundDag, origin: &StepCall) {
    let place = |file: Option<usize>, line: usize| match file {
        Some(file) => format!("{} line {line}", dag.pipeline_files[file].path),
        None => format!("line {line}"),
    };
    let mut chain = vec![&dag.calls[origin.call]];
    while let Some(parent) = chain.last().and_then(|call| call.parent) {
        chain.push(&dag.calls[parent]);
    }
    for call in chain.iter().rev() {
        write!(
            text,
            "{} = {} ({}), ",
            call.instance,
            call.operation,
            place(call.at_file, call.at_line)
        )
        .expect("writing to a String");
    }
    text.push_str(&place(chain[0].file, origin.line));
}

/// A call to an operation carried out by steps as written, its arguments
/// left out: `m, t = summarise(...)`.
pub fn render_call(call: &Call) -> String {
    format!("{} = {}(...)", call.outputs.join(", "), call.operation)
}

/// A command's arguments, each with its paths filled in and quoted as a
/// shell reads it, separated by spaces.
fn push_command(text: &mut String, dag: &BoundDag, command: &[Argument]) {
    let mut word = String::new();
    for (index, argument) in command.iter().enumerate() {
        word.clear();
        for part in argument {
            word.push_str(part.text(dag));
        }
        if index > 0 {
            text.push(' ');
        }
        text.push_str(&shell_word(&word));
    }
}

/// What can be made from a DAG's sources, what cannot, and why.
pub fn render_artifacts(pipeline: &Pipeline, report: &ArtifactReport) -> String {
    Report {
        pipeline,
        report,
        by_target: false,
    }
    .to_string()
}

/// The same report with the incomplete artifacts grouped by final target:
/// each incomplete artifact no other incomplete job needs, with the
/// incomplete artifacts it waits on nested under it, and each reason once.
pub fn render_artifacts_by_target(pipeline: &Pipeline, report: &ArtifactReport) -> String {
    Report {
        pipeline,
        report,
        by_target: true,
    }
    .to_string()
}

/// How many sources no job reads, and which: each by its identity when
/// there are at most three, as `1 source artifact is used by no job:
/// price[store=S07]`, else how many of each product in source order, as
/// `4 source artifacts are used by no job (calibration: 4)`; `None` when
/// every source is read.
pub fn unused_sources_summary(report: &ArtifactReport) -> Option<String> {
    let unused = report.unused_sources();
    if unused.is_empty() {
        return None;
    }
    let noun = if unused.len() == 1 {
        "artifact is"
    } else {
        "artifacts are"
    };
    if unused.len() <= 3 {
        let dag = &report.dag;
        let named: Vec<_> = unused
            .iter()
            .map(|&source| render_artifact(dag, dag.artifact(source)))
            .collect();
        return Some(format!(
            "{} source {noun} used by no job: {}",
            unused.len(),
            named.join(", ")
        ));
    }
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for &source in &unused {
        let product = report.dag.artifact(source).product;
        match counts.last_mut() {
            Some((last, count)) if *last == product => *count += 1,
            _ => counts.push((product, 1)),
        }
    }
    let by_product: Vec<_> = counts
        .iter()
        .map(|(product, count)| format!("{product}: {count}"))
        .collect();
    Some(format!(
        "{} source {noun} used by no job ({})",
        unused.len(),
        by_product.join(", ")
    ))
}

/// Writes jobs as the text reports show them, from a resolved or a bound
/// DAG, one piece at a time. Jobs are separated by a blank line.
#[derive(Default)]
struct JobWriter {
    text: String,
}

impl JobWriter {
    /// A job's number, stage and operation, up to its inputs.
    fn head(&mut self, id: JobId, stage: Option<&str>, operation: &str) {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        let text = &mut self.text;
        writeln!(text, "Job {id}").expect("writing to a String");
        if let Some(stage) = stage {
            writeln!(text, "  stage: {stage}").expect("writing to a String");
        }
        writeln!(text, "  operation: {operation}\n  inputs:").expect("writing to a String");
    }

    /// The heading of a job's outputs. A lone output is shown without its
    /// port.
    fn outputs(&mut self, single: bool) {
        self.text.push_str(if single {
            "  output:\n"
        } else {
            "  outputs:\n"
        });
    }

    /// The start of an artifact's line, with its port when shown.
    fn line(&mut self, port: Option<&str>) {
        self.text.push_str("    ");
        if let Some(port) = port {
            self.text.push_str(port);
            self.text.push_str(": ");
        }
    }

    /// The rest of an artifact's line: its identity and, unless unknown, its
    /// type.
    fn artifact(&mut self, identity: &str, artifact_type: &TypeExpr) {
        self.text.push_str(identity);
        push_type(&mut self.text, artifact_type);
        self.text.push('\n');
    }

    /// An artifact's path, with a `/` after a folder's.
    fn path(&mut self, path: &str, folder: bool) {
        let slash = if folder { "/" } else { "" };
        writeln!(self.text, "      path: {path}{slash}").expect("writing to a String");
    }

    /// The jobs this one depends on, if any.
    fn tail(&mut self, depends_on: &[JobId]) {
        let Some((first, rest)) = depends_on.split_first() else {
            return;
        };
        write!(self.text, "  depends_on: {first}").expect("writing to a String");
        for dependency in rest {
            write!(self.text, ", {dependency}").expect("writing to a String");
        }
        self.text.push('\n');
    }
}

/// A report, and the pipeline it was resolved from, which names the calls
/// its incomplete jobs came from.
struct Report<'a> {
    pipeline: &'a Pipeline,
    report: &'a ArtifactReport,
    /// Group the incomplete artifacts by final target, and count the
    /// complete ones without listing them.
    by_target: bool,
}

impl fmt::Display for Report<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let report = self.report;
        let dag = &report.dag;
        let held_back: BTreeSet<_> = report
            .coverage
            .iter()
            .flat_map(|gap| &gap.sources)
            .map(|source| (source.product.as_str(), &source.entities))
            .collect();
        let sources = report
            .sources
            .iter()
            .map(|&source| dag.artifact(source))
            .filter(|source| !held_back.contains(&(source.product, source.entities)))
            .map(|source| format!("{}  (source)", typed_artifact(dag, source)));
        let made = dag.jobs.iter().flat_map(|job| {
            let step = dag.step(job);
            job.outputs.iter().map(move |&artifact| {
                let stage = in_stage(step.stage.as_deref());
                let operation = &step.operation;
                let artifact = typed_artifact(dag, dag.artifact(artifact));
                format!("{artifact}  (job {}: {operation}{stage})", job.id)
            })
        });
        let complete: Vec<_> = sources.chain(made).collect();
        writeln!(f, "Complete artifacts: {}", complete.len())?;
        if !self.by_target {
            for line in complete {
                writeln!(f, "  {line}")?;
            }
        }
        if self.by_target {
            self.write_targets(f, &held_back)?;
        } else {
            self.write_incomplete(f, &held_back)?;
        }
        self.write_unused(f)?;
        self.write_coverage(f)
    }
}

impl Report<'_> {
    /// Each output that cannot be made, and each gap in its job's inputs.
    fn write_incomplete(
        &self,
        f: &mut fmt::Formatter<'_>,
        held_back: &BTreeSet<(&str, &EntityBinding)>,
    ) -> fmt::Result {
        let incomplete = &self.report.incomplete;
        let count: usize = incomplete.iter().map(|job| job.outputs.len()).sum();
        writeln!(f, "\nIncomplete artifacts: {count}")?;
        for job in incomplete {
            for artifact in &job.outputs {
                self.write_output(f, job, artifact, "  ")?;
            }
            for gap in &job.gaps {
                self.write_gap(f, gap, held_back, "    ")?;
            }
        }
        Ok(())
    }

    /// An incomplete output and the call it came from.
    fn write_output(
        &self,
        f: &mut fmt::Formatter<'_>,
        job: &IncompleteJob,
        artifact: &ArtifactInstance,
        indent: &str,
    ) -> fmt::Result {
        let stage = in_stage(job.stage.as_deref());
        let artifact = typed_artifact(&self.report.dag, artifact.view());
        write!(f, "{indent}{artifact}  ({}{stage}", job.operation)?;
        if let Some(call) = job.call.map(|call| self.pipeline.written_call(call)) {
            write!(
                f,
                ", in `{}` on line {}",
                render_call(call),
                call.place.line
            )?;
        }
        writeln!(f, ")")
    }

    /// One reason a job is incomplete.
    fn write_gap(
        &self,
        f: &mut fmt::Formatter<'_>,
        gap: &Gap,
        held_back: &BTreeSet<(&str, &EntityBinding)>,
        indent: &str,
    ) -> fmt::Result {
        match gap {
            Gap::Unmatched(error) => {
                // A reason's own lines after the first, as a near-miss hint,
                // are written to line up under a dash 4 columns in.
                // Keep in step with `ResolveError`'s `Display`, which writes
                // those lines 4 spaces in.
                if indent.len() == 4 {
                    return writeln!(f, "{indent}- {error}");
                }
                let error = error.to_string().replace("\n    ", &format!("\n{indent}"));
                writeln!(f, "{indent}- {error}")
            }
            Gap::Blocked { port, artifact } => {
                let key = (artifact.product.as_str(), &artifact.entities);
                let reason = if held_back.contains(&key) {
                    "a coverage gap holds back"
                } else {
                    "cannot be produced"
                };
                let artifact = render_artifact(&self.report.dag, artifact.view());
                writeln!(
                    f,
                    "{indent}- input `{port}` needs {artifact}, which {reason}"
                )
            }
        }
    }

    /// The sources no job reads, when there are any.
    fn write_unused(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unused = self.report.unused_sources();
        if unused.is_empty() {
            return Ok(());
        }
        let dag = &self.report.dag;
        writeln!(f, "\nUnused sources: {}", unused.len())?;
        for source in unused {
            writeln!(f, "  {}", typed_artifact(dag, dag.artifact(source)))?;
        }
        Ok(())
    }

    /// Each missing requirement, and the sources it holds back.
    fn write_coverage(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let coverage = &self.report.coverage;
        if coverage.is_empty() {
            return Ok(());
        }
        writeln!(f, "\nCoverage gaps: {}", coverage.len())?;
        for gap in coverage {
            writeln!(f, "  {}", gap.error)?;
            if !gap.sources.is_empty() {
                let sources: Vec<_> = gap
                    .sources
                    .iter()
                    .map(|source| render_artifact(&self.report.dag, source.view()))
                    .collect();
                writeln!(f, "    holds back: {}", sources.join(", "))?;
            }
        }
        Ok(())
    }
}

fn in_stage(stage: Option<&str>) -> String {
    stage.map_or_else(String::new, |stage| format!(", stage {stage}"))
}

/// `identity` with its type, unless the type is unknown.
fn typed(mut identity: String, artifact_type: &TypeExpr) -> String {
    push_type(&mut identity, artifact_type);
    identity
}

/// An artifact's type as every report shows it after the artifact: ` : Type`,
/// or nothing when the type is unknown.
fn push_type(text: &mut String, artifact_type: &TypeExpr) {
    if *artifact_type != TypeExpr::Unknown {
        write!(text, " : {artifact_type}").expect("writing to a String");
    }
}

fn typed_artifact(dag: &ResolvedDag, artifact: Artifact<'_>) -> String {
    typed(render_artifact(dag, artifact), artifact.artifact_type)
}

/// An artifact with its entities in its product's declared order.
fn render_artifact(dag: &ResolvedDag, artifact: Artifact<'_>) -> String {
    let Some(dimensions) = dag.product_dimensions.get(artifact.product) else {
        return artifact.to_string();
    };
    let entities = dimensions
        .iter()
        .filter_map(|dimension| Some((dimension.as_str(), artifact.entities.get(dimension)?)));
    identity(artifact.product, entities)
}
