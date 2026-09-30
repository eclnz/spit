//! Text reports of a resolved DAG: its jobs, and what can and cannot be made.

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use crate::command::shell_word;
use crate::model::{
    identity, push_identity, Artifact, ArtifactReport, EntityBinding, Gap, ResolvedDag,
};
use crate::spitdag::{ArgPart, Argument, BoundDag, BoundJob};
use crate::types::TypeExpr;

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
        writer.head(job.id, job.stage.as_deref(), &job.operation);
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
            writer.path(artifact.path);
        }
    };
    for job in &dag.jobs {
        writer.head(job.id, job.stage.as_deref(), &job.operation);
        for (port, artifacts) in &job.inputs {
            for &input in artifacts {
                artifact(&mut writer, Some(port), input);
            }
        }
        let single = job.outputs.len() == 1;
        writer.outputs(single);
        for (port, output) in &job.outputs {
            artifact(&mut writer, (!single).then_some(port.as_str()), *output);
        }
        writer.tail(&job.depends_on);
        if view.commands {
            push_command_lines(&mut writer.text, dag, job);
        }
    }
    writer.text
}

/// Each job as its heading, then its `verify` lines and its command line:
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
        write!(text, "Job {}  {}", job.id, job.operation).expect("writing to a String");
        if let Some(stage) = &job.stage {
            write!(text, "  [{stage}]").expect("writing to a String");
        }
        text.push('\n');
        push_command_lines(&mut text, dag, job);
    }
    text
}

/// A job's `verify` lines, then its `run` line.
fn push_command_lines(text: &mut String, dag: &BoundDag, job: &BoundJob) {
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
}

/// A command's arguments, each with its paths filled in and quoted as a
/// shell reads it, separated by spaces.
fn push_command(text: &mut String, dag: &BoundDag, command: &[Argument]) {
    let mut word = String::new();
    for (index, argument) in command.iter().enumerate() {
        word.clear();
        for part in argument {
            match part {
                ArgPart::Text(literal) => word.push_str(literal),
                ArgPart::Path(id) => word.push_str(dag.path(*id)),
            }
        }
        if index > 0 {
            text.push(' ');
        }
        text.push_str(&shell_word(&word));
    }
}

/// What can be made from a DAG's sources, what cannot, and why.
pub fn render_artifacts(report: &ArtifactReport) -> String {
    Report(report).to_string()
}

/// Writes jobs as the text reports show them, from a resolved or a bound
/// DAG, one piece at a time. Jobs are separated by a blank line.
#[derive(Default)]
struct JobWriter {
    text: String,
}

impl JobWriter {
    /// A job's number, stage and operation, up to its inputs.
    fn head(&mut self, id: usize, stage: Option<&str>, operation: &str) {
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

    fn path(&mut self, path: &str) {
        writeln!(self.text, "      path: {path}").expect("writing to a String");
    }

    /// The jobs this one depends on, if any.
    fn tail(&mut self, depends_on: &[usize]) {
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

struct Report<'a>(&'a ArtifactReport);

impl fmt::Display for Report<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let report = self.0;
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
            job.outputs.iter().map(move |&artifact| {
                let stage = in_stage(job.stage.as_deref());
                let operation = &job.operation;
                let artifact = typed_artifact(dag, dag.artifact(artifact));
                format!("{artifact}  (job {}: {operation}{stage})", job.id)
            })
        });
        let complete: Vec<_> = sources.chain(made).collect();
        writeln!(f, "Complete artifacts: {}", complete.len())?;
        for line in complete {
            writeln!(f, "  {line}")?;
        }
        self.write_incomplete(f, &held_back)?;
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
        let dag = &self.0.dag;
        let incomplete = &self.0.incomplete;
        let count: usize = incomplete.iter().map(|job| job.outputs.len()).sum();
        writeln!(f, "\nIncomplete artifacts: {count}")?;
        for job in incomplete {
            let stage = in_stage(job.stage.as_deref());
            for artifact in &job.outputs {
                let artifact = typed_artifact(dag, artifact.view());
                writeln!(f, "  {artifact}  ({}{stage})", job.operation)?;
            }
            for gap in &job.gaps {
                match gap {
                    Gap::Unmatched(error) => writeln!(f, "    - {error}")?,
                    Gap::Blocked { port, artifact } => {
                        let key = (artifact.product.as_str(), &artifact.entities);
                        let reason = if held_back.contains(&key) {
                            "a coverage gap holds back"
                        } else {
                            "cannot be produced"
                        };
                        let artifact = render_artifact(dag, artifact.view());
                        writeln!(f, "    - input `{port}` needs {artifact}, which {reason}")?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Each missing requirement, and the sources it holds back.
    fn write_coverage(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let coverage = &self.0.coverage;
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
                    .map(|source| render_artifact(&self.0.dag, source.view()))
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
