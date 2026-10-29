//! Paths bound to the artifacts of resolved jobs, and the files they name.

use std::fmt;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::product::PathBinder;
use super::rules::inspect_paths;
use super::template::{error, require_directory, PathError};
use crate::model::{
    Artifact, ArtifactId, ArtifactKey, BesideFile, EntityBinding, Pipeline, PipelineIndex,
    ResolvedDag,
};

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
///
/// Keep in step with `bind_dag_with`, which trusts that the rules were
/// checked and the paths bound here as `bind_dag` would: if this stops
/// checking them, that function must start.
pub fn validate_bound_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<(VerifiedFiles, BoundPaths), PathError> {
    require_directory(root)?;
    check_rules(pipeline, dag)?;
    let paths = bound_paths(pipeline, dag)?;
    let made = outputs_made(dag);
    // What the jobs read and none makes, the sources, checked in artifact
    // order so the first missing file is reported.
    let mut needed: Vec<_> = with_paths(dag, &paths)
        .filter(|(id, _, _)| !made[id.index()])
        .map(|(_, artifact, relative)| (artifact.key(), relative))
        .collect();
    needed.sort_unstable_by_key(|&(artifact, _)| artifact);
    let index = PipelineIndex::new(pipeline);
    for &(artifact, relative) in &needed {
        let full_path = root.join(relative);
        let (present, kind) = if index.is_folder(artifact.0) {
            (full_path.is_dir(), "folder")
        } else {
            (full_path.is_file(), "file")
        };
        if !present {
            return Err(error(format!(
                "missing source {kind} for `{}[{}]`: `{}`",
                artifact.0,
                artifact.1,
                full_path.display()
            )));
        }
    }
    let verified = VerifiedFiles {
        sources: needed.len(),
    };
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
}

impl fmt::Display for VerifiedFiles {
    /// Reads as `3 source files verified.`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} source files verified.", self.sources)
    }
}

/// Check that every product of `pipeline` has a valid path rule, and that
/// each in `dag` has one, except sources whose files the inventory gave.
pub(crate) fn check_rules(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<(), PathError> {
    inspect_paths(pipeline)?
        .with_inventory_paths(dag.located_products())
        .validate(dag.artifacts.products().map(|(product, _)| product))
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
    let mut paths = vec![None; dag.artifacts.len()];
    let mut owners: FxHashMap<String, ArtifactId> = FxHashMap::default();
    let mut own = |id: ArtifactId, relative: String, paths: &mut Vec<Option<String>>| {
        if let Some(previous) = owners.insert(relative.clone(), id) {
            let (previous, artifact) = (dag.artifact(previous), dag.artifact(id));
            return Err(error(format!(
                "artifacts `{}[{}]` and `{}[{}]` bind to the same path `{relative}`",
                previous.product, previous.entities, artifact.product, artifact.entities
            )));
        }
        paths[id.index()] = Some(relative);
        Ok(())
    };
    // An output written beside one of its job's inputs lies beside the file
    // that input has, whatever gave it, so it is bound once the others are.
    let beside: Vec<Option<BesideFile<'_>>> = dag
        .artifacts
        .products()
        .map(|(product, _)| {
            binder
                .index()
                .beside(product)
                .filter(|beside| beside.input.is_some())
        })
        .collect();
    let mut companions = Vec::new();
    let mut deferred = vec![false; dag.artifacts.len()];
    for job in &dag.jobs {
        for id in job.input_artifacts().chain(job.outputs.iter().copied()) {
            if paths[id.index()].is_some() || deferred[id.index()] {
                continue;
            }
            let number = dag.artifacts.product_of(id);
            if let Some(beside) = beside[number as usize] {
                let input = beside
                    .input
                    .and_then(|input| job.inputs.get(input)?.first());
                if let (Some(&input), true) = (input, job.outputs.contains(&id)) {
                    companions.push((id, input, beside));
                    deferred[id.index()] = true;
                    continue;
                }
            }
            let artifact = dag.artifact(id);
            let relative = match dag.source_path(id) {
                Some(path) => path.to_owned(),
                None => {
                    binder.bind_numbered(number, artifact, || format!("path for `{artifact}`"))?
                }
            };
            own(id, relative, &mut paths)?;
        }
    }
    // Jobs come after the jobs they read from, so a file beside a file
    // beside another finds its input's path bound.
    for (id, input, beside) in companions {
        let relative = beside_path(dag, &paths, id, input, beside)?;
        own(id, relative, &mut paths)?;
    }
    // The first such path in path order is the one reported. Paths share
    // their directories, so a directory found to hold no artifact, nor any
    // above it, is kept, and a path in it needs no more looking up.
    let index = binder.index();
    let mut made: Option<Vec<bool>> = None;
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
        // Few paths are inside another, so only they are judged.
        .filter_map(|(relative, artifact, directory, other)| {
            let folder = index.is_folder(dag.artifact(other).product);
            let made = made.get_or_insert_with(|| outputs_made(dag));
            let (inner_made, outer_made) = (made[artifact.index()], made[other.index()]);
            // Nothing writes a source, so a source may sit in a source folder.
            (!folder || inner_made || outer_made)
                .then_some((relative, artifact, directory, other, folder, outer_made))
        })
        .min_by(|left, right| left.0.cmp(right.0));
    if let Some((_, artifact, directory, other, folder, outer_made)) = enclosed {
        let (artifact, other) = (dag.artifact(artifact), dag.artifact(other));
        let what = match (folder, outer_made) {
            (false, _) => "which is a file",
            (true, true) => "a folder its job writes whole",
            (true, false) => "a source folder, which no job may write inside",
        };
        return Err(error(format!(
            "path of `{}[{}]` puts it inside `{directory}`, the path of `{}[{}]`, {what}",
            artifact.product, artifact.entities, other.product, other.entities
        )));
    }
    Ok(paths)
}

/// The path of `id`, written beside `input`, whose path is bound: the
/// input's path with `beside`'s suffix in place of its extension, or after
/// its whole name.
fn beside_path(
    dag: &ResolvedDag,
    paths: &[Option<String>],
    id: ArtifactId,
    input: ArtifactId,
    beside: BesideFile<'_>,
) -> Result<String, PathError> {
    let artifact = dag.artifact(id);
    let sibling = paths[input.index()].as_deref().ok_or_else(|| {
        error(format!(
            "no path is bound for `{}`, which `{artifact}` is written beside",
            dag.artifact(input)
        ))
    })?;
    let stem = match beside.replaces.filter(|_| !beside.append) {
        Some(extension) => sibling.strip_suffix(extension).ok_or_else(|| {
            error(format!(
                "`{artifact}` is written beside `{sibling}`, which does not end with `{extension}` for `{}` to replace; append to its whole name with `+{}`",
                beside.suffix, beside.suffix
            ))
        })?,
        None => sibling,
    };
    Ok(format!("{stem}{}", beside.suffix))
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
            .push((artifact.key(), path));
    }
    let owned = |((product, entities), path): (Key<'_>, &String)| {
        ((product.to_owned(), entities.clone()), path.clone())
    };
    let mut collisions = Vec::new();
    for mut group in folded.into_values().filter(|group| group.len() > 1) {
        group.sort_unstable_by_key(|&(artifact, _)| artifact);
        let first = group[0];
        collisions.extend(group[1..].iter().map(|&later| [owned(first), owned(later)]));
    }
    collisions.sort_by(|[_, (left, _)], [_, (right, _)]| left.cmp(right));
    collisions
}
