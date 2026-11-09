//! Resolve jobs: expand each step of a compiled pipeline, in dependency
//! order, into concrete jobs over an inventory's artifacts.

mod matching;
mod reviews;

use crate::model::ResolveError;
use rustc_hash::FxHashSet;

use crate::model::{
    ArtifactId, ArtifactInstance, ArtifactReport, ArtifactType, DagStep, Gap, IncompleteJob,
    Invocation, JobId, OperationDef, Pipeline, PipelineIndex, ResolvedDag, SourceInventory, StepId,
    UnmatchedVerdicts, Withheld,
};
use crate::review::{port_reviews, ReviewIndex};

use crate::compile::{compile, CompiledPipeline, CompiledStep};

use self::matching::{expand_step, make_job, Expansion, Held};
use self::reviews::{Judging, Verdicts};

/// Resolve every job of `pipeline` over the sources in `inventory`, failing
/// on the first job that cannot be made.
pub fn resolve(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ResolvedDag, ResolveError> {
    let report = resolve_artifacts_excluding(pipeline, inventory, &[])?;
    match first_failure(&report.incomplete) {
        Some(error) => Err(error),
        None => Ok(report.dag),
    }
}

/// Why the first of `incomplete` cannot be made; a blocked gap always
/// follows the gap that blocks it, so there is one whenever any job is
/// incomplete.
pub(crate) fn first_failure(incomplete: &[IncompleteJob]) -> Option<ResolveError> {
    incomplete
        .iter()
        .flat_map(|job| &job.gaps)
        .find_map(|gap| match gap {
            Gap::Unmatched(error) => Some(error.clone()),
            Gap::Blocked { .. } => None,
        })
}

/// Resolve what can be made, reporting each job that cannot and why. Some
/// sources may be known to be unusable, such as those a missing requirement
/// holds back; jobs that need them are reported as blocked.
pub fn resolve_artifacts_excluding(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    unavailable: &[ArtifactInstance],
) -> Result<ArtifactReport, ResolveError> {
    resolve_artifacts_with_partial(pipeline, inventory, unavailable, false)
}

/// Resolve all complete jobs, allowing an aggregate's many input to use
/// complete members when other members cannot be produced.
pub fn resolve_artifacts_partial(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    unavailable: &[ArtifactInstance],
) -> Result<ArtifactReport, ResolveError> {
    resolve_artifacts_with_partial(pipeline, inventory, unavailable, true)
}

fn resolve_artifacts_with_partial(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    unavailable: &[ArtifactInstance],
    partial: bool,
) -> Result<ArtifactReport, ResolveError> {
    let CompiledPipeline { steps } = compile(pipeline)?;
    let index = PipelineIndex::new(pipeline);
    let reviewed = ReviewIndex::new(&index);
    let verdicts = Verdicts::new(&inventory.verdicts);
    let mut resolution = Resolution {
        families: Vec::new(),
        incomplete: Vec::new(),
        withheld: Vec::new(),
        any_withheld: false,
        withheld_report: Vec::new(),
        held_seen: FxHashSet::default(),
        reviewed: &reviewed,
        verdicts: &verdicts,
        producers: Vec::new(),
        dag: ResolvedDag {
            product_dimensions: pipeline
                .products
                .iter()
                .map(|product| (product.name.clone(), product.dimensions.clone()))
                .collect(),
            ..ResolvedDag::default()
        },
        incomplete_jobs: Vec::new(),
    };
    pipeline.check_sources(inventory, |product, record| {
        let number = resolution.product(&product.name, &product.artifact_type);
        match resolution
            .dag
            .artifacts
            .add(number, record.entities.clone())
        {
            Ok(id) => {
                resolution.families[number as usize].push(id);
                true
            }
            Err(_) => false,
        }
    })?;
    // Sources in natural order, by product in declaration order.
    let mut sources = Vec::new();
    for product in &pipeline.products {
        if let Some(number) = resolution.dag.artifacts.product_number(&product.name) {
            let artifacts = &resolution.dag.artifacts;
            let family = &mut resolution.families[number as usize];
            family.sort_by(|&left, &right| {
                artifacts
                    .entities(left)
                    .cmp_in(artifacts.entities(right), &product.dimensions)
            });
            sources.extend_from_slice(family);
        }
    }
    resolution.grow();
    for &source in &sources {
        resolution.judge_product(source);
    }
    for artifact in unavailable {
        if let Some(id) = resolution
            .dag
            .artifacts
            .find(&artifact.product, &artifact.entities)
        {
            resolution.incomplete[id.index()] = true;
        }
    }
    for step in &steps {
        let id = StepId::new(resolution.dag.steps.len());
        resolution.dag.steps.push(DagStep {
            operation: step.operation.name.clone(),
            outputs: step.invocation.outputs.clone(),
            stage: step.invocation.stage.clone(),
            origin: step.invocation.origin.clone(),
            line: step.invocation.line,
        });
        let outputs: Vec<u32> = step
            .outputs
            .iter()
            .map(|(product, artifact_type)| resolution.product(&product.name, artifact_type))
            .collect();
        let judging = Judging {
            withheld: &resolution.withheld,
            verdicts: &verdicts,
            ports: if pipeline.reviews.is_empty() {
                vec![Vec::new(); step.operation.inputs.len()]
            } else {
                (0..step.operation.inputs.len())
                    .map(|port| port_reviews(step.operation, step.invocation, port))
                    .collect()
            },
            any_withheld: resolution.any_withheld,
        };
        let expansions = expand_step(
            step,
            &resolution.dag.artifacts,
            &resolution.families,
            &resolution.incomplete,
            partial,
            &judging,
        );
        for expansion in expansions {
            resolution.add(step.invocation, step.operation, id, &outputs, expansion)?;
        }
        for ((product, _), &number) in step.outputs.iter().zip(&outputs) {
            let artifacts = &resolution.dag.artifacts;
            resolution.families[number as usize].sort_by(|&left, &right| {
                artifacts
                    .entities(left)
                    .cmp_in(artifacts.entities(right), &product.dimensions)
            });
        }
    }
    let unmatched_verdicts = unmatched_verdicts(&resolution, &steps, inventory);
    let mut dag = resolution.dag;
    dag.locate_sources(inventory);
    Ok(ArtifactReport {
        sources,
        dag,
        incomplete: resolution.incomplete_jobs,
        coverage: Vec::new(),
        withheld: resolution.withheld_report,
        unmatched_verdicts,
    })
}

/// The rows of each review file that cover no artifact the review judges,
/// file by file in the order the inventory lists them.
fn unmatched_verdicts(
    resolution: &Resolution<'_>,
    steps: &[CompiledStep<'_>],
    inventory: &SourceInventory,
) -> Vec<UnmatchedVerdicts> {
    if inventory.verdicts.is_empty() {
        return Vec::new();
    }
    let artifacts = &resolution.dag.artifacts;
    let mut matched = vec![false; inventory.verdicts.len()];
    let mut mark = |product: &str, review: &str| {
        let Some(number) = artifacts.product_number(product) else {
            return;
        };
        for &artifact in &resolution.families[number as usize] {
            resolution
                .verdicts
                .mark(review, artifacts.entities(artifact), &mut matched);
        }
    };
    for (product, _) in artifacts.products() {
        for review in resolution.reviewed.of(product) {
            mark(product, review);
        }
    }
    for step in steps {
        for (port, binding) in step.invocation.inputs.iter().enumerate() {
            for review in port_reviews(step.operation, step.invocation, port) {
                mark(binding.product_name(), review);
            }
        }
    }
    let mut files: Vec<UnmatchedVerdicts> = Vec::new();
    for (row, matched) in inventory.verdicts.iter().zip(matched) {
        if matched {
            continue;
        }
        match files.iter_mut().find(|known| known.file == row.file) {
            Some(known) => known.lines.push(row.line),
            None => files.push(UnmatchedVerdicts {
                file: row.file.clone(),
                lines: vec![row.line],
            }),
        }
    }
    files
}

/// The artifacts and jobs found so far, as steps are expanded in order.
/// Artifacts are kept in the DAG's table; the rest is by artifact id or
/// product number.
struct Resolution<'a> {
    /// Each product's artifacts, by product number, in natural order.
    families: Vec<Vec<ArtifactId>>,
    /// Artifacts that will not exist: sources the caller rules out, and
    /// the outputs of incomplete jobs.
    incomplete: Vec<bool>,
    /// Artifacts no reader may use, though they may exist: those a review on
    /// their product fails or holds, and the outputs of jobs left out for
    /// reading one. Unlike `incomplete`, this is no error.
    withheld: Vec<bool>,
    /// Whether any artifact is withheld so far.
    any_withheld: bool,
    /// Each reviewed artifact a review withholds, as a note says it.
    withheld_report: Vec<Withheld>,
    /// The artifacts a review on an input port has been noted for, by step
    /// and port, so a step that reads one many times notes it once.
    held_seen: FxHashSet<(StepId, usize, ArtifactId)>,
    /// The reviews on each product's artifacts for every reader.
    reviewed: &'a ReviewIndex<'a>,
    verdicts: &'a Verdicts<'a>,
    /// The job that makes each output artifact.
    producers: Vec<Option<JobId>>,
    dag: ResolvedDag,
    incomplete_jobs: Vec<IncompleteJob>,
}

impl Resolution<'_> {
    /// The number of `product`, with a family for it.
    fn product(&mut self, product: &str, artifact_type: &ArtifactType) -> u32 {
        let number = self.dag.artifacts.product(product, artifact_type);
        if self.families.len() <= number as usize {
            self.families.resize_with(number as usize + 1, Vec::new);
        }
        number
    }

    /// Give every artifact in the table its flags.
    fn grow(&mut self) {
        let len = self.dag.artifacts.len();
        self.incomplete.resize(len, false);
        self.withheld.resize(len, false);
        self.producers.resize(len, None);
    }

    /// Withhold `artifact` from every reader when a review on its product
    /// fails it or has no verdict for it, and note why.
    fn judge_product(&mut self, artifact: ArtifactId) {
        let view = self.dag.artifacts.get(artifact);
        for &review in self.reviewed.of(view.product) {
            let Some(judged) = self.verdicts.judge(review, view.entities) else {
                continue;
            };
            self.withheld_report.push(Withheld {
                artifact: view.to_instance(),
                review: review.to_owned(),
                port: None,
                outcome: judged,
            });
            self.withheld[artifact.index()] = true;
            self.any_withheld = true;
        }
    }

    /// Note each artifact a review on an input port kept from a job of
    /// `step`, once however many jobs read it.
    fn note_held(&mut self, step: StepId, operation: &OperationDef, held: Vec<Held>) {
        for held in held {
            if !self.held_seen.insert((step, held.port, held.artifact)) {
                continue;
            }
            self.withheld_report.push(Withheld {
                artifact: self.dag.artifacts.get(held.artifact).to_instance(),
                review: held.review,
                port: Some((
                    operation.name.clone(),
                    operation.inputs[held.port].name.clone(),
                )),
                outcome: held.judged,
            });
        }
    }

    /// Record one expanded job: a complete job joins the DAG, one with gaps
    /// is reported with its outputs marked incomplete, and one a review
    /// leaves out makes nothing, its outputs withheld in turn.
    fn add(
        &mut self,
        invocation: &Invocation,
        operation: &OperationDef,
        step: StepId,
        products: &[u32],
        expansion: Expansion,
    ) -> Result<(), ResolveError> {
        let mut outputs = Vec::with_capacity(products.len());
        for &product in products {
            let id = self
                .dag
                .artifacts
                .add(product, expansion.context.clone())
                .map_err(|existing| ResolveError::DuplicateOutputArtifact {
                    artifact: self.dag.artifacts.get(existing).to_instance(),
                })?;
            self.families[product as usize].push(id);
            outputs.push(id);
        }
        self.grow();
        self.note_held(step, operation, expansion.held);
        if expansion.left_out {
            for output in &outputs {
                self.withheld[output.index()] = true;
            }
            self.any_withheld = true;
        } else if expansion.gaps.is_empty() {
            let job = make_job(
                JobId::new(self.dag.jobs.len() + 1),
                step,
                expansion.inputs,
                outputs,
                &self.producers,
            );
            for output in &job.outputs {
                self.producers[output.index()] = Some(job.id);
            }
            let made = job.outputs.clone();
            self.dag.jobs.push(job);
            for output in made {
                self.judge_product(output);
            }
        } else {
            for output in &outputs {
                self.incomplete[output.index()] = true;
            }
            self.incomplete_jobs.push(IncompleteJob {
                operation: operation.name.clone(),
                stage: invocation.stage.clone(),
                inputs: expansion.inputs.into_iter().flatten().collect(),
                outputs: outputs
                    .iter()
                    .map(|&id| self.dag.artifacts.get(id).to_instance())
                    .collect(),
                gaps: expansion.gaps,
                call: invocation.origin.as_ref().map(|origin| origin.call),
            });
        }
        Ok(())
    }
}
