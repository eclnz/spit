//! Each artifact a job uses, as JSON: written once however many jobs use it.

use std::ops::Range;

use crate::json::{write_array, write_string};
use crate::model::{ArtifactId, Under};
use crate::types::TypeExpr;

use super::{BoundArtifact, BoundDag};

/// Each artifact a job uses, as JSON, written once however many jobs use it.
pub(super) struct ArtifactJson {
    text: String,
    /// Where each artifact's JSON is in `text`, by id; empty for one no job
    /// uses.
    spans: Vec<Range<usize>>,
}

impl ArtifactJson {
    pub(super) fn new(dag: &BoundDag) -> Self {
        let mut used = vec![false; dag.paths.len()];
        for job in &dag.jobs {
            for id in job.input_artifacts().chain(job.outputs.iter().copied()) {
                used[id.index()] = true;
            }
        }
        let mut text = String::new();
        let mut spans = Vec::new();
        for id in dag.artifacts.ids() {
            let start = text.len();
            if used[id.index()] {
                write_artifact(&mut text, &dag.artifact(id));
            }
            spans.push(start..text.len());
        }
        Self { text, spans }
    }

    pub(super) fn get(&self, id: ArtifactId) -> &str {
        &self.text[self.spans[id.index()].clone()]
    }
}

pub(super) fn write_artifact(out: &mut String, artifact: &BoundArtifact<'_>) {
    out.push_str("{\"product\":");
    write_string(out, artifact.product);
    out.push_str(",\"entities\":{");
    for (index, (dimension, value)) in artifact.entities().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(out, dimension);
        out.push(':');
        write_string(out, value);
    }
    out.push('}');
    out.push_str(",\"type\":");
    write_type(out, artifact.artifact_type);
    out.push_str(",\"path\":");
    write_string(out, artifact.path);
    out.push_str(match artifact.under {
        Under::Root => ",\"under\":\"root\"",
        Under::OutputRoot => ",\"under\":\"output_root\"",
    });
    out.push_str(if artifact.folder {
        ",\"kind\":\"folder\"}"
    } else {
        ",\"kind\":\"file\"}"
    });
}

fn write_type(out: &mut String, artifact_type: &TypeExpr) {
    match artifact_type {
        TypeExpr::Unknown => out.push_str("null"),
        TypeExpr::Variable(name) => {
            out.push_str("{\"variable\":");
            write_string(out, name);
            out.push('}');
        }
        TypeExpr::Named(name) => {
            out.push_str("{\"name\":");
            write_string(out, name);
            out.push_str(",\"args\":[]}");
        }
        TypeExpr::Applied { constructor, args } => {
            out.push_str("{\"name\":");
            write_string(out, constructor);
            out.push_str(",\"args\":");
            write_array(out, args, write_type);
            out.push('}');
        }
    }
}
