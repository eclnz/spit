use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::model::{ArtifactId, JobId, Pipeline, PipelineIndex, ResolvedDag, ReviewRecord, Under};
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
        if let Some(used) = pipeline
            .invocations
            .iter()
            .flat_map(|invocation| &invocation.reviews)
            .find(|used| used.evidence.is_some())
        {
            return Err(error(format!(
                "cannot save review `{}` with explicit evidence; saved DAGs do not carry review evidence",
                used.name
            )));
        }
        let index = PipelineIndex::new(pipeline);
        let reviews = ReviewIndex::new(&index);
        let attached = attached_products(&index, &reviews);
        let mut targets: BTreeMap<&str, BTreeSet<BoundTarget>> = BTreeMap::new();
        for id in dag.artifacts.ids() {
            let artifact = dag.artifacts.get(id);
            for &name in reviews.of(artifact.product) {
                if self.paths[id.index()].is_empty() {
                    let Some(path) = dag.source_path(id) else {
                        continue;
                    };
                    path.clone_into(&mut self.paths[id.index()]);
                    self.under[id.index()] = Under::Root;
                }
                targets.entry(name).or_default().insert(BoundTarget {
                    artifact: id,
                    input: None,
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
