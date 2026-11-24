use std::fmt;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::product::PathBinder;
use super::rules::inspect_paths;
use crate::model::{
    Artifact, ArtifactId, ArtifactKey, BesideFile, EntityBinding, Pipeline, PipelineIndex,
    ResolvedDag, Under,
};
use crate::template::{error, require_directory, PathError};

pub(crate) const HERE_WITHOUT_FILE: &str =
    "a `here` source is found from the pipeline's folder, so it needs a pipeline read from a file";

pub fn validate_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<VerifiedFiles, PathError> {
    validate_bound_source_files(pipeline, dag, root).map(|(verified, _)| verified)
}

pub fn validate_bound_source_files(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    root: &Path,
) -> Result<(VerifiedFiles, BoundPaths), PathError> {
    require_directory(root)?;
    check_rules(pipeline, dag)?;
    let bound = bound_paths(pipeline, dag)?;
    let made = outputs_made(dag);
    let mut needed: Vec<_> = with_paths(dag, &bound.paths)
        .filter(|(id, _, _)| !made[id.index()])
        .map(|(id, artifact, relative)| (artifact.key(), relative, bound.under[id.index()]))
        .collect();
    needed.sort_unstable_by_key(|&(artifact, _, _)| artifact);
    let index = PipelineIndex::new(pipeline);
    for &(artifact, relative, under) in &needed {
        let base = if under == Under::Pipeline {
            Path::new(
                pipeline
                    .folder
                    .as_deref()
                    .ok_or_else(|| error(HERE_WITHOUT_FILE))?,
            )
        } else {
            root
        };
        let full_path = base.join(relative);
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
    Ok((verified, bound))
}

#[derive(Clone, Debug)]
pub struct BoundPaths {
    pub(crate) paths: Vec<Option<String>>,
    pub(crate) under: Vec<Under>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VerifiedFiles {
    pub sources: usize,
}

impl fmt::Display for VerifiedFiles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} source files verified.", self.sources)
    }
}

pub(crate) fn check_rules(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<(), PathError> {
    inspect_paths(pipeline)?
        .with_inventory_paths(dag.located_products())
        .validate(dag.artifacts.products().map(|(product, _)| product))
}

pub(crate) fn outputs_made(dag: &ResolvedDag) -> Vec<bool> {
    let mut made = vec![false; dag.artifacts.len()];
    for &output in dag
        .jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .chain(dag.views.keys())
    {
        made[output.index()] = true;
    }
    made
}

pub(crate) fn bound_paths(pipeline: &Pipeline, dag: &ResolvedDag) -> Result<BoundPaths, PathError> {
    let mut binder = PathBinder::new(pipeline);
    let mut paths = vec![None; dag.artifacts.len()];
    let mut under = vec![Under::Root; dag.artifacts.len()];
    for &output in dag
        .jobs
        .iter()
        .flat_map(|job| &job.outputs)
        .chain(dag.views.keys())
    {
        under[output.index()] = Under::OutputRoot;
    }
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
            if binder
                .index()
                .product(artifact.product)
                .is_some_and(|product| product.here.is_some())
            {
                under[id.index()] = Under::Pipeline;
            }
            let relative = match dag.source_path(id) {
                Some(path) => path.to_owned(),
                None => {
                    binder.bind_numbered(number, artifact, || format!("path for `{artifact}`"))?
                }
            };
            own(id, relative, &mut paths)?;
        }
    }
    for (id, input, beside) in companions {
        under[id.index()] = under[input.index()];
        if under[id.index()] == Under::Pipeline {
            return Err(error(format!(
                "`{}` is written beside `{}`, a `here` source, but jobs write nothing in the pipeline's folder",
                dag.artifact(id).product,
                dag.artifact(input).product
            )));
        }
        let relative = beside_path(dag, &paths, id, input, beside)?;
        own(id, relative, &mut paths)?;
    }
    for &view in dag.views.keys() {
        if paths[view.index()].is_none() {
            let artifact = dag.artifact(view);
            let relative =
                binder.bind_numbered(dag.artifacts.product_of(view), artifact, || {
                    format!("path for `{artifact}`")
                })?;
            own(view, relative, &mut paths)?;
        }
    }
    let index = binder.index();
    check_directory_views(dag, &paths, &under)?;
    let mut made: Option<Vec<bool>> = None;
    let mut clear: FxHashSet<&str> = FxHashSet::default();
    let enclosed = owners
        .iter()
        .filter_map(|(relative, &artifact)| {
            let parent = &relative[..relative.rfind('/')?];
            if clear.contains(parent) {
                return None;
            }
            let mut has_owner = false;
            let invalid = relative.match_indices('/').find_map(|(end, _)| {
                let (directory, &other) = owners.get_key_value(&relative[..end])?;
                has_owner = true;
                if let Some(members) = dag.views.get(&other) {
                    return members.binary_search(&artifact).is_err().then_some((
                        relative,
                        artifact,
                        directory,
                        other,
                        "a directory view that does not list it",
                    ));
                }
                let folder = index.is_folder(dag.artifact(other).product);
                let made = made.get_or_insert_with(|| outputs_made(dag));
                let what = match (folder, made[other.index()]) {
                    (false, _) => "which is a file",
                    (true, true) => "a folder its job writes whole",
                    (true, false) if made[artifact.index()] => {
                        "a source folder, which no job may write inside"
                    }
                    (true, false) => return None,
                };
                Some((relative, artifact, directory, other, what))
            });
            if !has_owner {
                clear.insert(parent);
            }
            invalid
        })
        .min_by(|left, right| left.0.cmp(right.0));
    if let Some((_, artifact, directory, other, what)) = enclosed {
        let (artifact, other) = (dag.artifact(artifact), dag.artifact(other));
        return Err(error(format!(
            "path of `{}[{}]` puts it inside `{directory}`, the path of `{}[{}]`, {what}",
            artifact.product, artifact.entities, other.product, other.entities
        )));
    }
    Ok(BoundPaths { paths, under })
}

fn check_directory_views(
    dag: &ResolvedDag,
    paths: &[Option<String>],
    under: &[Under],
) -> Result<(), PathError> {
    for (&view, members) in &dag.views {
        let folder = paths[view.index()]
            .as_deref()
            .expect("view output has a path");
        for &member in members {
            let path = paths[member.index()]
                .as_deref()
                .expect("view input has a path");
            if under[member.index()] != under[view.index()]
                || !path
                    .strip_prefix(folder)
                    .is_some_and(|tail| tail.starts_with('/'))
            {
                return Err(error(format!(
                    "`{}` at `{path}` must be inside directory view `{}` at `{folder}` under the same root",
                    dag.artifact(member), dag.artifact(view)
                )));
            }
        }
    }
    Ok(())
}

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

fn with_paths<'a>(
    dag: &'a ResolvedDag,
    paths: &'a [Option<String>],
) -> impl Iterator<Item = (ArtifactId, Artifact<'a>, &'a String)> {
    dag.artifacts
        .ids()
        .zip(paths)
        .filter_map(|(id, path)| Some((id, dag.artifact(id), path.as_ref()?)))
}

type Key<'a> = (&'a str, &'a EntityBinding);

pub(crate) fn case_collisions(
    dag: &ResolvedDag,
    paths: &[Option<String>],
) -> Vec<[(ArtifactKey, String); 2]> {
    let mut folded: FxHashMap<String, Vec<(Key<'_>, &String)>> = FxHashMap::default();
    for (_, artifact, path) in with_paths(dag, paths) {
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

pub(crate) struct Misread {
    pub(crate) artifact: ArtifactKey,
    pub(crate) path: String,
    pub(crate) read: EntityBinding,
}

pub(crate) fn misread_sources(
    pipeline: &Pipeline,
    dag: &ResolvedDag,
    paths: &[Option<String>],
) -> Vec<Misread> {
    let mut binder = PathBinder::new(pipeline);
    let mut misread = Vec::new();
    for (_, artifact, path) in with_paths(dag, paths) {
        if !binder.index().is_source(artifact.product) {
            continue;
        }
        let Some(read) = binder.read_back(artifact.product, path) else {
            continue;
        };
        if read != *artifact.entities {
            misread.push(Misread {
                artifact: (artifact.product.to_owned(), artifact.entities.clone()),
                path: path.clone(),
                read,
            });
        }
    }
    misread
}
