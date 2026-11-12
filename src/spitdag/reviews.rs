//! The reviews of a bound DAG: each review's file, and the artifacts it
//! withheld when SPIT made the plan, with the outputs each one kept from it.

use std::path::{Component, Path, PathBuf};

use crate::model::{
    ArtifactId, ArtifactInstance, ArtifactReport, Judged, Pipeline, PipelineIndex, ReviewRecord,
    Under, VerdictRow,
};
use crate::review::{attached_products, ReviewIndex};

use super::BoundDag;

/// A review as the plan applied it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundReview {
    pub name: String,
    /// The review's file as an absolute path, or `None` when the recipe
    /// names none.
    pub file: Option<String>,
    /// Whether the file existed when the plan was made.
    pub found: bool,
    /// How many rows the file holds.
    pub verdicts: usize,
    /// The lines of the rows that match no artifact.
    pub unmatched: Vec<usize>,
    /// Each artifact the review withheld, in the order the resolver met them.
    pub targets: Vec<BoundTarget>,
}

/// An artifact a review withheld: a person inspects it to give a verdict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundTarget {
    /// The artifact, with its path bound even when no job reads it.
    pub artifact: ArtifactId,
    /// The operation and input port the review is attached to; `None` for a
    /// source or an output.
    pub port: Option<(String, String)>,
    /// The row that failed it, or `None` when no row judges it yet.
    pub rejected: Option<Rejection>,
    /// The outputs not planned because of it.
    pub withholds: Vec<ArtifactInstance>,
    /// The outputs of jobs planned without it.
    pub shrinks: Vec<ArtifactInstance>,
}

/// The row that gave an artifact the verdict `fail`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rejection {
    pub reason: Option<String>,
    /// The row's line in the review's file.
    pub line: usize,
}

impl BoundDag {
    /// Record each review `pipeline` attaches to a product, in declaration
    /// order, with its file from `files`, its rows from `verdicts`, and what
    /// `report` says it withheld. A withheld source no job reads takes its
    /// path from `report`'s DAG, whose artifacts this DAG's are.
    pub fn add_reviews(
        &mut self,
        pipeline: &Pipeline,
        report: &ArtifactReport,
        files: &[ReviewRecord],
        verdicts: &[VerdictRow],
    ) {
        if pipeline.reviews.is_empty() {
            return;
        }
        let index = PipelineIndex::new(pipeline);
        let attached = attached_products(&index, &ReviewIndex::new(&index));
        for review in &pipeline.reviews {
            let name = review.name.as_str();
            if !attached.contains_key(name) {
                continue;
            }
            let record = files.iter().find(|file| file.review == name);
            let rows: Vec<&VerdictRow> = verdicts.iter().filter(|row| row.review == name).collect();
            let unmatched = rows
                .first()
                .and_then(|row| {
                    report
                        .unmatched_verdicts
                        .iter()
                        .find(|unmatched| unmatched.file == row.file)
                })
                .map_or_else(Vec::new, |unmatched| unmatched.lines.clone());
            let mut targets = Vec::new();
            for withheld in report
                .withheld
                .iter()
                .filter(|withheld| withheld.review == name)
            {
                let Some(id) = self
                    .artifacts
                    .find(&withheld.artifact.product, &withheld.artifact.entities)
                else {
                    continue;
                };
                if self.paths[id.index()].is_empty() {
                    if let Some(path) = report.dag.source_path(id) {
                        path.clone_into(&mut self.paths[id.index()]);
                        self.under[id.index()] = Under::Root;
                    }
                }
                targets.push(BoundTarget {
                    artifact: id,
                    port: withheld.port.clone(),
                    rejected: match &withheld.outcome {
                        Judged::Rejected { reason, line, .. } => Some(Rejection {
                            reason: reason.clone(),
                            line: *line,
                        }),
                        Judged::Held => None,
                    },
                    withholds: withheld.withholds.clone(),
                    shrinks: withheld.shrinks.clone(),
                });
            }
            self.reviews.push(BoundReview {
                name: name.to_owned(),
                file: record.map(|record| absolute(&record.file)),
                found: record.is_some_and(|record| record.found),
                verdicts: rows.len(),
                unmatched,
                targets,
            });
        }
    }
}

/// `path` as an absolute path with no `.` or `..` in it, or as it is when
/// it has none. Its folders are not read, so a file that does not exist yet
/// has a path too.
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
