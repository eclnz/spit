use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::model::{
    ArtifactId, JobId, Pipeline, PipelineIndex, Port, ResolvedDag, ReviewRecord, Under,
};
use crate::review::{attached_products, port_reviews, ReviewIndex};
use crate::template::{error, PathError};

use super::BoundDag;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundReview {
    pub name: String,
    pub file: Option<String>,
    pub dimensions: Vec<String>,
    pub targets: Vec<BoundTarget>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BoundTarget {
    pub artifact: ArtifactId,
    pub input: Option<(JobId, usize)>,
    pub evidence: Option<ArtifactId>,
}

impl BoundDag {
    pub fn add_reviews(
        &mut self,
        pipeline: &Pipeline,
        dag: &ResolvedDag,
        files: &[ReviewRecord],
    ) -> Result<(), PathError> {
        if pipeline.reviews.is_empty() {
            return Ok(());
        }
        let index = PipelineIndex::new(pipeline);
        let reviews = ReviewIndex::new(&index);
        let attached = attached_products(&index, &reviews);
        let mut evidence_by_product = FxHashMap::default();
        for invocation in &pipeline.invocations {
            for used in &invocation.reviews {
                if let (Port::Output(port), Some(evidence)) = (used.port, used.evidence.as_deref())
                {
                    if let Some((candidate, evidence)) =
                        invocation.outputs.get(port).zip(index.product(evidence))
                    {
                        evidence_by_product
                            .insert((candidate.as_str(), used.name.as_str()), evidence);
                    }
                }
            }
        }
        let mut targets: BTreeMap<&str, BTreeSet<BoundTarget>> = BTreeMap::new();
        for id in dag.artifacts.ids() {
            let artifact = dag.artifacts.get(id);
            for &name in reviews.of(artifact.product) {
                if !self.locate(dag, id) {
                    continue;
                }
                let evidence =
                    if let Some(product) = evidence_by_product.get(&(artifact.product, name)) {
                        let id = artifact
                            .entities
                            .project(&product.dimensions)
                            .and_then(|entities| dag.artifacts.find(&product.name, &entities))
                            .filter(|&id| self.locate(dag, id))
                            .ok_or_else(|| {
                                error(format!(
                            "review `{name}` of `{artifact}` has no matching evidence from `{}`",
                            product.name
                        ))
                            })?;
                        Some(id)
                    } else {
                        None
                    };
                targets.entry(name).or_default().insert(BoundTarget {
                    artifact: id,
                    input: None,
                    evidence,
                });
            }
        }
        let ports: Vec<_> = dag
            .steps
            .iter()
            .map(|step| {
                let Some(operation) = index.operation(&step.operation) else {
                    return Vec::new();
                };
                let Some((invocation, _)) = step
                    .outputs
                    .first()
                    .and_then(|output| index.producer(output))
                else {
                    return Vec::new();
                };
                (0..operation.inputs.len())
                    .map(|port| port_reviews(operation, invocation, port))
                    .collect::<Vec<_>>()
            })
            .collect();
        for job in &self.jobs {
            for (port, names) in ports[job.step.index()].iter().enumerate() {
                for &name in names {
                    targets
                        .entry(name)
                        .or_default()
                        .extend(job.inputs[port].iter().map(|&artifact| BoundTarget {
                            artifact,
                            input: Some((job.id, port)),
                            evidence: None,
                        }));
                }
            }
        }
        for review in &pipeline.reviews {
            let name = review.name.as_str();
            let Some(products) = attached.get(name) else {
                continue;
            };
            let dimensions = products
                .first()
                .into_iter()
                .flat_map(|product| &product.dimensions)
                .filter(|dimension| {
                    products
                        .iter()
                        .all(|product| product.dimensions.contains(dimension))
                })
                .cloned()
                .collect();
            let file = files
                .iter()
                .find(|file| file.review == name)
                .map(|file| absolute(&file.file));
            let root = self
                .output_root
                .as_deref()
                .map_or_else(|| Path::new("."), Path::new);
            if let Some(file) = &file {
                for record in self.screen_records.values() {
                    if file.eq_ignore_ascii_case(&absolute(&root.join(record))) {
                        return Err(error(format!(
                            "screen record would replace review file `{file}`"
                        )));
                    }
                }
            }
            self.reviews.push(BoundReview {
                name: name.to_owned(),
                file,
                dimensions,
                targets: targets
                    .remove(name)
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            });
        }
        Ok(())
    }

    fn locate(&mut self, dag: &ResolvedDag, id: ArtifactId) -> bool {
        if self.paths[id.index()].is_empty() {
            let Some(path) = dag.source_path(id) else {
                return false;
            };
            path.clone_into(&mut self.paths[id.index()]);
            self.under[id.index()] = Under::Root;
        }
        true
    }
}

fn absolute(path: &Path) -> String {
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut clean = PathBuf::new();
    for component in full.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other),
        }
    }
    clean.to_string_lossy().into_owned()
}
