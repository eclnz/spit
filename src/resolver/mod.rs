mod matching;

use crate::model::ResolveError;

use crate::model::{
    ArtifactId, ArtifactInstance, ArtifactReport, ArtifactType, DagStep, Gap, IncompleteJob,
    Invocation, JobId, OperationDef, Pipeline, ResolvedDag, SourceInventory, StepId,
};

use crate::compile::{compile, CompiledPipeline};

use self::matching::{expand_step, make_job, Expansion};

pub fn resolve(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
) -> Result<ResolvedDag, ResolveError> {
    let report = resolve_potential_artifacts(pipeline, inventory, &[], false)?;
    match first_failure(&report.incomplete) {
        Some(error) => Err(error),
        None => Ok(report.dag),
    }
}

pub(crate) fn first_failure(incomplete: &[IncompleteJob]) -> Option<ResolveError> {
    incomplete
        .iter()
        .flat_map(|job| &job.gaps)
        .find_map(|gap| match gap {
            Gap::Unmatched(error) => Some(error.clone()),
            Gap::Blocked { .. } => None,
        })
}

pub fn resolve_potential_artifacts(
    pipeline: &Pipeline,
    inventory: &SourceInventory,
    unavailable: &[ArtifactInstance],
    partial: bool,
) -> Result<ArtifactReport, ResolveError> {
    let CompiledPipeline { steps } = compile(pipeline)?;
    let mut resolution = Resolution {
        families: Vec::new(),
        incomplete: Vec::new(),
        producers: Vec::new(),
        defer_missing: !partial
            && (!pipeline.reviews.is_empty()
                || pipeline
                    .checks
                    .iter()
                    .any(|check| check.kind == crate::model::CheckKind::Screen)),
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
            collection: step.shape.collection.clone(),
        });
        let outputs: Vec<u32> = step
            .outputs
            .iter()
            .map(|(product, artifact_type)| resolution.product(&product.name, artifact_type))
            .collect();
        let expansions = expand_step(
            step,
            &resolution.dag.artifacts,
            &resolution.families,
            &resolution.incomplete,
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
    let mut dag = resolution.dag;
    dag.locate_sources(inventory);
    Ok(ArtifactReport {
        sources,
        dag,
        incomplete: resolution.incomplete_jobs,
        coverage: Vec::new(),
    })
}

struct Resolution {
    families: Vec<Vec<ArtifactId>>,
    incomplete: Vec<bool>,
    producers: Vec<Option<JobId>>,
    defer_missing: bool,
    dag: ResolvedDag,
    incomplete_jobs: Vec<IncompleteJob>,
}

impl Resolution {
    fn product(&mut self, product: &str, artifact_type: &ArtifactType) -> u32 {
        let number = self.dag.artifacts.product(product, artifact_type);
        if self.families.len() <= number as usize {
            self.families.resize_with(number as usize + 1, Vec::new);
        }
        number
    }

    fn grow(&mut self) {
        let len = self.dag.artifacts.len();
        self.incomplete.resize(len, false);
        self.producers.resize(len, None);
    }

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
        let missing = self
            .defer_missing
            .then(|| missing_inputs(operation, &expansion.gaps, &expansion.members))
            .flatten();
        if expansion.gaps.is_empty() && operation.outputs[0].view {
            let view = outputs[0];
            let mut members: Vec<_> = expansion.inputs.into_iter().flatten().collect();
            members.sort_unstable();
            self.dag.views.insert(view, members);
        } else if expansion.gaps.is_empty() || missing.is_some() {
            let mut job = make_job(
                JobId::new(self.dag.jobs.len() + 1),
                step,
                expansion.inputs,
                outputs,
                &self.producers,
            );
            job.missing = missing.unwrap_or_default();
            if !self.dag.views.is_empty() {
                let member_producers = job
                    .inputs
                    .iter()
                    .flatten()
                    .filter_map(|input| self.dag.views.get(input))
                    .flatten()
                    .filter_map(|member| self.producers[member.index()]);
                job.dependencies.extend(member_producers);
                job.dependencies.sort_unstable();
                job.dependencies.dedup();
            }
            for output in &job.outputs {
                self.producers[output.index()] = Some(job.id);
            }
            self.dag.jobs.push(job);
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

fn missing_inputs(
    operation: &OperationDef,
    gaps: &[Gap],
    members: &[(usize, Vec<String>)],
) -> Option<Vec<crate::model::MissingInput>> {
    if operation.outputs[0].view {
        return None;
    }
    gaps.iter()
        .enumerate()
        .map(|(index, gap)| {
            let Gap::Unmatched(error @ ResolveError::MissingInput { .. }) = gap else {
                return None;
            };
            let member = members
                .iter()
                .find(|(gap, _)| *gap == index)
                .map(|(_, member)| member.clone());
            Some(crate::model::MissingInput {
                message: error.to_string(),
                member,
            })
        })
        .collect()
}
