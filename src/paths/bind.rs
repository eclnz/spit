//! Paths bound to the artifacts of resolved jobs, and the files they name.

use std::fmt;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::rules::inspect_paths;
use super::template::{error, require_directory, PathBinder, PathError};
use crate::model::{Artifact, ArtifactId, ArtifactKey, EntityBinding, Pipeline, ResolvedDag};

/// Check the files needed to start the resolved DAG under a dataset root.
/// Derived outputs are deliberately excluded because the pipeline creates them.
pub fn validate_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<VerifiedFiles, PathError> {
    validate_bound_source_files(pipeline, dag, root).map(|(verified, _)| verified)
}

/// As [`validate_source_files`], with the paths it bound, so that
/// [`bind_dag_with`](crate::bind_dag_with) need not bind them again.
pub fn validate_bound_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<(VerifiedFiles, BoundPaths), PathError> {
    require_directory(root)?;
    check_rules(pipeline, dag)?;
    let paths = bound_paths(pipeline, dag)?;
    let made = outputs_made(dag);
    // What the jobs read and none makes, checked in artifact order so the
    // first missing file is reported.
    let mut needed: Vec<_> = with_paths(dag, &paths)
        .filter(|(id, _, _)| !made[id.index()])
        .map(|(_, artifact, relative)| (key(artifact), relative))
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
    Ok((verified, BoundPaths(paths)))
}

/// Each artifact's path in a resolved DAG, as [`validate_bound_source_files`]
/// bound them.
#[derive(Clone, Debug)]
pub struct BoundPaths(pub(crate) Vec<Option<String>>);

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
        .with_inventory_paths(dag.located_products())
        .validate(false)
}

/// Whether each artifact, by id, is made by one of the resolved jobs.
pub(crate) fn outputs_made(dag: &ResolvedDag) -> Vec<bool> {
    let mut made = vec![false; dag.artifacts.len()];
    for &output in dag.jobs.iter().flat_map(|job| &job.outputs) {
        made[output.index()] = true;
    }
    made
}

/// The path of each artifact the jobs use, by artifact id; `None` for an
/// artifact no job uses.
pub(crate) fn bound_paths(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
) -> Result<Vec<Option<String>>, PathError> {
    let mut binder = PathBinder::new(pipeline);
    // Each product's dimensions, found once, by product number.
    let mut dimensions: Vec<Option<&[String]>> = vec![None; dag.artifacts.products().count()];
    let mut paths = vec![None; dag.artifacts.len()];
    let mut owners: FxHashMap<String, ArtifactId> = FxHashMap::default();
    for id in dag
        .jobs
        .iter()
        .flat_map(|job| job.input_artifacts().chain(job.outputs.iter().copied()))
    {
        if paths[id.index()].is_some() {
            continue;
        }
        let artifact = dag.artifact(id);
        let number = dag.artifacts.product_of(id);
        let dimensions = match dimensions[number as usize] {
            Some(dimensions) => dimensions,
            None => {
                let found = dag
                    .product_dimensions
                    .get(artifact.product)
                    .ok_or_else(|| error(format!("unknown product `{}`", artifact.product)))?;
                *dimensions[number as usize].insert(found)
            }
        };
        let relative = match dag.source_path(id) {
            Some(path) => path.to_owned(),
            None => binder.bind_numbered(number, dimensions, artifact, || {
                format!("path for `{artifact}`")
            })?,
        };
        if let Some(previous) = owners.insert(relative.clone(), id) {
            let previous = dag.artifact(previous);
            return Err(error(format!(
                "artifacts `{}[{}]` and `{}[{}]` bind to the same path `{relative}`",
                previous.product, previous.entities, artifact.product, artifact.entities
            )));
        }
        paths[id.index()] = Some(relative);
    }
    // The first such path in path order is the one reported. Paths share
    // their directories, so a directory found to hold no file, nor any above
    // it, is kept, and a path in it needs no more looking up.
    let mut clear: FxHashSet<&str> = FxHashSet::default();
    let enclosed = owners
        .iter()
        .filter_map(|(relative, &artifact)| {
            let parent = &relative[..relative.rfind('/')?];
            if clear.contains(parent) {
                return None;
            }
            let found = relative
                .match_indices('/')
                .find_map(|(end, _)| owners.get_key_value(&relative[..end]));
            let Some((directory, &other)) = found else {
                clear.insert(parent);
                return None;
            };
            Some((relative, artifact, directory, other))
        })
        .min_by(|left, right| left.0.cmp(right.0));
    if let Some((_, artifact, directory, other)) = enclosed {
        let (artifact, other) = (dag.artifact(artifact), dag.artifact(other));
        return Err(error(format!(
            "path of `{}[{}]` puts it inside `{directory}`, the path of `{}[{}]`, which is a file",
            artifact.product, artifact.entities, other.product, other.entities
        )));
    }
    Ok(paths)
}

/// Each artifact the jobs use, with its path, in no order.
fn with_paths<'a>(
    dag: &'a ResolvedDag,
    paths: &'a [Option<String>],
) -> impl Iterator<Item = (ArtifactId, Artifact<'a>, &'a String)> {
    dag.artifacts
        .ids()
        .zip(paths)
        .filter_map(|(id, path)| Some((id, dag.artifact(id), path.as_ref()?)))
}

/// An artifact by its product and entities, as [`ArtifactKey`] without copies.
type Key<'a> = (&'a str, &'a EntityBinding);

fn key(artifact: Artifact<'_>) -> Key<'_> {
    (artifact.product, artifact.entities)
}

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
    let mut folded: FxHashMap<String, Vec<(Key<'_>, &String)>> = FxHashMap::default();
    for (_, artifact, path) in with_paths(dag, &paths) {
        folded
            .entry(path.to_lowercase())
            .or_default()
            .push((key(artifact), path));
    }
    let owned = |((product, entities), path): (Key<'_>, &String)| {
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
