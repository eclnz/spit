//! Paths bound to the artifacts of resolved jobs, and the files they name.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use rustc_hash::FxHashMap;

use super::rules::inspect_paths;
use super::template::{error, require_directory, PathBinder, PathError};
use crate::model::{
    ArtifactInstance, ArtifactKey, ArtifactMap, ArtifactSet, EntityBinding, Pipeline, ResolvedDag,
};

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
    // What the jobs read and none makes, checked in artifact order so the
    // first missing file is reported.
    let mut needed: Vec<_> = paths
        .iter()
        .filter(|(product, entities, _)| outputs.get_by(product, entities).is_none())
        .map(|(product, entities, relative)| ((product, entities), relative))
        .collect();
    needed.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    let mut verified = VerifiedFiles::default();
    for (artifact, relative) in needed {
        // In a DAG cut to one stage, as by `ResolvedDag::only_stage`, what
        // other stages make must already exist.
        let made_by = pipeline
            .invocations
            .iter()
            .find(|invocation| invocation.outputs.iter().any(|output| output == artifact.0));
        let full_path = root.join(relative);
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
pub(crate) fn output_keys(dag: &ResolvedDag) -> ArtifactSet {
    dag.jobs.iter().flat_map(|job| &job.outputs).collect()
}

pub(crate) fn bound_paths(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
) -> Result<ArtifactMap<String>, PathError> {
    // Only these products' artifacts can have files the inventory gave.
    let located: BTreeSet<&str> = dag
        .source_paths
        .keys()
        .map(|(product, _)| product.as_str())
        .collect();
    let mut binder = PathBinder::new(pipeline);
    let mut paths = ArtifactMap::default();
    let mut owners: FxHashMap<String, &ArtifactInstance> = FxHashMap::default();
    for artifact in dag
        .jobs
        .iter()
        .flat_map(|job| job.input_artifacts().chain(&job.outputs))
    {
        if paths.contains(artifact) {
            continue;
        }
        let dimensions = dag
            .product_dimensions
            .get(&artifact.product)
            .ok_or_else(|| error(format!("unknown product `{}`", artifact.product)))?;
        let given = located
            .contains(artifact.product.as_str())
            .then(|| dag.source_paths.get(&artifact.key()))
            .flatten();
        let relative = match given {
            Some(path) => path.clone(),
            None => binder.bind(dimensions, artifact, || format!("path for `{artifact}`"))?,
        };
        if let Some(previous) = owners.insert(relative.clone(), artifact) {
            return Err(error(format!(
                "artifacts `{}[{}]` and `{}[{}]` bind to the same path `{relative}`",
                previous.product, previous.entities, artifact.product, artifact.entities
            )));
        }
        paths.insert(artifact, relative);
    }
    // The first such path in path order is the one reported.
    let enclosed = owners
        .iter()
        .filter_map(|(relative, artifact)| {
            let (directory, other) = relative
                .match_indices('/')
                .find_map(|(end, _)| owners.get_key_value(&relative[..end]))?;
            Some((relative, artifact, directory, other))
        })
        .min_by(|left, right| left.0.cmp(right.0));
    if let Some((_, artifact, directory, other)) = enclosed {
        return Err(error(format!(
            "path of `{}[{}]` puts it inside `{directory}`, the path of `{}[{}]`, which is a file",
            artifact.product, artifact.entities, other.product, other.entities
        )));
    }
    Ok(paths)
}

/// An artifact by its product and entities, as [`ArtifactKey`] without copies.
type Artifact<'a> = (&'a str, &'a EntityBinding);

/// Pairs of artifacts, with their paths, whose paths differ only in case:
/// in artifact order, the first artifact with each such path paired with
/// each later one.
pub(crate) fn case_collisions(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
) -> Vec<[(ArtifactKey, String); 2]> {
    let Ok(paths) = bound_paths(pipeline, dag) else {
        return Vec::new();
    };
    let mut folded: FxHashMap<String, Vec<(Artifact<'_>, &String)>> = FxHashMap::default();
    for (product, entities, path) in paths.iter() {
        folded
            .entry(path.to_lowercase())
            .or_default()
            .push(((product, entities), path));
    }
    let owned = |((product, entities), path): (Artifact<'_>, &String)| {
        ((product.to_owned(), entities.clone()), path.clone())
    };
    let mut collisions = Vec::new();
    for mut group in folded.into_values().filter(|group| group.len() > 1) {
        group.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        let first = group[0];
        collisions.extend(group[1..].iter().map(|&later| [owned(first), owned(later)]));
    }
    collisions.sort_by(|[_, (left, _)], [_, (right, _)]| left.cmp(right));
    collisions
}
