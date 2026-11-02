//! Each artifact a job uses, as JSON: written once however many jobs use it,
//! and in the form a fingerprint hashes.

use std::ops::Range;

use crate::json::{write_array, write_string};
use crate::model::ArtifactId;
use crate::types::TypeExpr;

use super::{BoundArtifact, BoundDag};

/// Each artifact a job uses, as JSON, written once however many jobs use it.
pub(super) struct ArtifactJson {
    text: String,
    /// Where each artifact's JSON is in `text`, by id; empty for one no job
    /// uses.
    spans: Vec<Range<usize>>,
    /// Where `,"type":...` is in each artifact's JSON, by id, so that the
    /// fingerprint can leave it out.
    types: Vec<Range<usize>>,
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
        let mut types = Vec::new();
        for id in dag.artifacts.ids() {
            let start = text.len();
            let type_span = if used[id.index()] {
                write_artifact(&mut text, &dag.artifact(id))
            } else {
                start..start
            };
            spans.push(start..text.len());
            types.push(type_span);
        }
        Self { text, spans, types }
    }

    pub(super) fn get(&self, id: ArtifactId) -> &str {
        &self.text[self.spans[id.index()].clone()]
    }

    /// The artifact as `get` has it, without its type. A type is checked
    /// when SPIT compiles and is in no command, so it is no part of the work
    /// a fingerprint identifies.
    pub(super) fn push_identity(&self, out: &mut String, id: ArtifactId) {
        let span = &self.spans[id.index()];
        let type_span = &self.types[id.index()];
        out.push_str(&self.text[span.start..type_span.start]);
        out.push_str(&self.text[type_span.end..span.end]);
    }
}

pub(super) fn write_artifact(out: &mut String, artifact: &BoundArtifact<'_>) -> Range<usize> {
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
    let type_start = out.len();
    out.push_str(",\"type\":");
    write_type(out, artifact.artifact_type);
    let type_end = out.len();
    out.push_str(",\"path\":");
    write_string(out, artifact.path);
    out.push_str(if artifact.folder {
        ",\"kind\":\"folder\"}"
    } else {
        ",\"kind\":\"file\"}"
    });
    type_start..type_end
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
