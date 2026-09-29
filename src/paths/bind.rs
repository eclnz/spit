//! Paths bound to the artifacts of resolved jobs, and the files they name.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use super::rules::inspect_paths;
use super::template::{bind_path, enclosing_path, error, require_directory, PathError};
use crate::model::{ArtifactInstance, ArtifactKey, Pipeline, ResolvedDag};

/// Check the files needed to start the resolved DAG under a dataset root.
/// Derived outputs are deliberately excluded because the pipeline creates them.
pub fn validate_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<VerifiedFiles, PathError> {
    require_directory(root)?;
    check_rules(pipeline, dag)?;
    let paths = bound_paths(pipeline, dag)?;
    let outputs = output_keys(dag);
    let mut verified = VerifiedFiles::default();
    for (artifact, relative) in paths {
        if outputs.contains(&artifact) {
            continue;
        }
        // With `--stage`, what other stages make must already exist.
        let made_by = pipeline
            .invocations
            .iter()
            .find(|invocation| invocation.outputs.contains(&artifact.0));
        let full_path = root.join(&relative);
        if !full_path.is_file() {
            return Err(error(match made_by {
                Some(invocation) => format!(
                    "missing file for `{}[{}]`, which {} makes: `{}`",
                    artifact.0,
                    artifact.1,
                    invocation.stage.as_ref().map_or_else(
                        || "an earlier step".to_owned(),
                        |stage| format!("stage `{stage}`")
                    ),
                    full_path.display()
                ),
                None => format!(
                    "missing source file for `{}[{}]`: `{}`",
                    artifact.0,
                    artifact.1,
                    full_path.display()
                ),
            }));
        }
        if made_by.is_some() {
            verified.made_elsewhere += 1;
        } else {
            verified.sources += 1;
        }
    }
    Ok(verified)
}

/// The files [`validate_source_files`] found under the root.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VerifiedFiles {
    /// Files of source products.
    pub sources: usize,
    /// Outputs of steps whose jobs the DAG leaves out, such as another
    /// stage's when one stage is selected.
    pub made_elsewhere: usize,
}

impl fmt::Display for VerifiedFiles {
    /// Reads as `3 source files verified.`, naming only counts above zero.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.sources > 0 || self.made_elsewhere == 0 {
            parts.push(format!("{} source files", self.sources));
        }
        if self.made_elsewhere > 0 {
            parts.push(format!(
                "{} files made outside the stage",
                self.made_elsewhere
            ));
        }
        write!(f, "{} verified.", parts.join(" and "))
    }
}

/// Check that every product of `pipeline` has a valid path rule, except
/// sources whose files the inventory gave the DAG.
pub(crate) fn check_rules(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<(), PathError> {
    inspect_paths(pipeline)?
        .with_inventory_paths(dag.source_paths.keys().map(|(product, _)| product.as_str()))
        .validate(false)
}

/// Every artifact the resolved jobs produce.
pub(crate) fn output_keys(dag: &ResolvedDag) -> BTreeSet<ArtifactKey> {
    dag.jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .map(ArtifactInstance::key)
        .collect()
}

pub(crate) fn bound_paths(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
) -> Result<BTreeMap<ArtifactKey, String>, PathError> {
    let mut paths = BTreeMap::new();
    let mut owners = BTreeMap::new();
    for artifact in dag
        .jobs
        .iter()
        .flat_map(|job| job.input_artifacts().chain(&job.outputs))
    {
        let identity = artifact.key();
        if paths.contains_key(&identity) {
            continue;
        }
        let dimensions = dag
            .product_dimensions
            .get(&artifact.product)
            .ok_or_else(|| error(format!("unknown product `{}`", artifact.product)))?;
        let relative = match dag.source_paths.get(&identity) {
            Some(path) => path.clone(),
            None => bind_path(pipeline, dimensions, artifact, || {
                format!("path for `{artifact}`")
            })?,
        };
        if let Some(previous) = owners.insert(relative.clone(), identity.clone()) {
            return Err(error(format!(
                "artifacts `{}[{}]` and `{}[{}]` bind to the same path `{relative}`",
                previous.0, previous.1, identity.0, identity.1
            )));
        }
        paths.insert(identity, relative);
    }
    for (relative, identity) in &owners {
        if let Some((directory, other)) = enclosing_path(&owners, relative) {
            return Err(error(format!(
                "path of `{}[{}]` puts it inside `{directory}`, the path of `{}[{}]`, which is a file",
                identity.0, identity.1, other.0, other.1
            )));
        }
    }
    Ok(paths)
}

/// Pairs of artifacts, with their paths, whose paths differ only in case.
pub(crate) fn case_collisions(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
) -> Vec<[(ArtifactKey, String); 2]> {
    let Ok(paths) = bound_paths(pipeline, dag) else {
        return Vec::new();
    };
    let mut folded: BTreeMap<String, (ArtifactKey, String)> = BTreeMap::new();
    let mut collisions = Vec::new();
    for (key, path) in paths {
        match folded.get(&path.to_lowercase()) {
            Some(first) => collisions.push([first.clone(), (key, path)]),
            None => {
                folded.insert(path.to_lowercase(), (key, path));
            }
        }
    }
    collisions
}
