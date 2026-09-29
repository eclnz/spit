//! Text reports of a resolved DAG: its jobs, and what can and cannot be made.

use std::collections::BTreeSet;
use std::fmt;

use crate::model::{identity, ArtifactInstance, ArtifactReport, Gap, ResolvedDag};
use crate::spitdag::{BoundArtifact, BoundDag};
use crate::types::TypeExpr;

/// The jobs as text, without ports or paths.
pub fn render_dag(dag: &ResolvedDag) -> String {
    let artifact = |artifact| Line {
        port: None,
        artifact: typed(render_artifact(dag, artifact), &artifact.artifact_type),
        path: None,
    };
    let jobs = dag.jobs.iter().map(|job| JobText {
        id: job.id,
        stage: job.stage.as_deref(),
        operation: &job.operation,
        inputs: job.input_artifacts().map(artifact).collect(),
        outputs: job.outputs.iter().map(artifact).collect(),
        depends_on: &job.dependencies,
    });
    Jobs(jobs.collect()).to_string()
}

/// The jobs of a bound DAG as text, each artifact with its port, and with
/// its path when `paths` is set.
pub fn render_bound_dag(dag: &BoundDag, paths: bool) -> String {
    let artifact = |port: &'_ str, artifact: &'_ BoundArtifact| Line {
        port: Some(port.to_owned()),
        artifact: typed(artifact.identity(), &artifact.artifact_type),
        path: paths.then(|| artifact.path.clone()),
    };
    let jobs = dag.jobs.iter().map(|job| JobText {
        id: job.id,
        stage: job.stage.as_deref(),
        operation: &job.operation,
        inputs: job
            .inputs
            .iter()
            .flat_map(|(port, artifacts)| artifacts.iter().map(|each| artifact(port, each)))
            .collect(),
        outputs: job
            .outputs
            .iter()
            .map(|(port, each)| artifact(port, each))
            .collect(),
        depends_on: &job.depends_on,
    });
    Jobs(jobs.collect()).to_string()
}

/// What can be made from a DAG's sources, what cannot, and why.
pub fn render_artifacts(report: &ArtifactReport) -> String {
    Report(report).to_string()
}

/// A job as the text reports show it, from a resolved or a bound DAG.
struct JobText<'a> {
    id: usize,
    stage: Option<&'a str>,
    operation: &'a str,
    inputs: Vec<Line>,
    outputs: Vec<Line>,
    depends_on: &'a [usize],
}

/// One artifact of a job: its port and path when shown.
struct Line {
    port: Option<String>,
    artifact: String,
    path: Option<String>,
}

struct Jobs<'a>(Vec<JobText<'a>>);

impl fmt::Display for Jobs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, job) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{job}")?;
        }
        Ok(())
    }
}

impl fmt::Display for JobText<'_> {
    /// A lone output is shown without its port.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Job {}", self.id)?;
        if let Some(stage) = self.stage {
            writeln!(f, "  stage: {stage}")?;
        }
        writeln!(f, "  operation: {}", self.operation)?;
        writeln!(f, "  inputs:")?;
        for line in &self.inputs {
            line.write(f, true)?;
        }
        let single = self.outputs.len() == 1;
        writeln!(f, "  {}:", if single { "output" } else { "outputs" })?;
        for line in &self.outputs {
            line.write(f, !single)?;
        }
        if !self.depends_on.is_empty() {
            let dependencies: Vec<_> = self.depends_on.iter().map(ToString::to_string).collect();
            writeln!(f, "  depends_on: {}", dependencies.join(", "))?;
        }
        Ok(())
    }
}

impl Line {
    fn write(&self, f: &mut fmt::Formatter<'_>, with_port: bool) -> fmt::Result {
        match self.port.as_deref().filter(|_| with_port) {
            Some(port) => writeln!(f, "    {port}: {}", self.artifact)?,
            None => writeln!(f, "    {}", self.artifact)?,
        }
        if let Some(path) = &self.path {
            writeln!(f, "      path: {path}")?;
        }
        Ok(())
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
            .collect();
        let sources = report
            .sources
            .iter()
            .filter(|source| !held_back.contains(source))
            .map(|source| format!("{}  (source)", typed_artifact(dag, source)));
        let made = dag.jobs.iter().flat_map(|job| {
            job.outputs.iter().map(move |artifact| {
                let stage = in_stage(job.stage.as_deref());
                let operation = &job.operation;
                let artifact = typed_artifact(dag, artifact);
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
        held_back: &BTreeSet<&ArtifactInstance>,
    ) -> fmt::Result {
        let dag = &self.0.dag;
        let incomplete = &self.0.incomplete;
        let count: usize = incomplete.iter().map(|job| job.outputs.len()).sum();
        writeln!(f, "\nIncomplete artifacts: {count}")?;
        for job in incomplete {
            let stage = in_stage(job.stage.as_deref());
            for artifact in &job.outputs {
                let artifact = typed_artifact(dag, artifact);
                writeln!(f, "  {artifact}  ({}{stage})", job.operation)?;
            }
            for gap in &job.gaps {
                match gap {
                    Gap::Unmatched(error) => writeln!(f, "    - {error}")?,
                    Gap::Blocked { port, artifact } => {
                        let reason = if held_back.contains(artifact) {
                            "a coverage gap holds back"
                        } else {
                            "cannot be produced"
                        };
                        let artifact = render_artifact(dag, artifact);
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
                    .map(|source| render_artifact(&self.0.dag, source))
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
fn typed(identity: String, artifact_type: &TypeExpr) -> String {
    if *artifact_type == TypeExpr::Unknown {
        identity
    } else {
        format!("{identity} : {artifact_type}")
    }
}

fn typed_artifact(dag: &ResolvedDag, artifact: &ArtifactInstance) -> String {
    typed(render_artifact(dag, artifact), &artifact.artifact_type)
}

/// An artifact with its entities in its product's declared order.
fn render_artifact(dag: &ResolvedDag, artifact: &ArtifactInstance) -> String {
    let Some(dimensions) = dag.product_dimensions.get(&artifact.product) else {
        return artifact.to_string();
    };
    let entities = dimensions.iter().filter_map(|dimension| {
        let value = artifact.entities.0.get(dimension)?;
        Some((dimension.as_str(), value.as_str()))
    });
    identity(&artifact.product, entities)
}
