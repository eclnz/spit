//! `here` sources: files kept beside the pipeline rather than in the
//! dataset, as a script a command runs is. Each is one file, found from the
//! pipeline's folder rather than read from the dataset.

use std::path::Path;

use super::Discovery;
use crate::model::{ArtifactInstance, EntityBinding, Pipeline, SourceRecord};
use crate::paths::{PathBinder, HERE_WITHOUT_FILE};
use crate::template::{error, PathError};

/// Add a record for each `here` source of `pipeline` to what `discovery`
/// found under `root`, with its path from the pipeline's folder. A file that
/// is missing is an error. A pipeline kept in its dataset has its `here`
/// sources under `root` too, and they are not files a source rule missed.
pub(super) fn append(
    pipeline: &Pipeline,
    root: &Path,
    discovery: &mut Discovery,
) -> Result<(), PathError> {
    let root = std::fs::canonicalize(root).ok();
    let mut binder = PathBinder::new(pipeline);
    for product in pipeline
        .products
        .iter()
        .filter(|product| product.here.is_some())
    {
        let folder = pipeline
            .folder
            .as_deref()
            .ok_or_else(|| error(HERE_WITHOUT_FILE))?;
        let artifact = ArtifactInstance::new(
            &product.name,
            product.artifact_type.clone(),
            EntityBinding::default(),
        );
        let relative = binder.bind(artifact.view(), || {
            format!("`here` source `{}`", product.name)
        })?;
        let full = Path::new(folder).join(&relative);
        if !full.is_file() {
            return Err(error(format!(
                "`here` source `{}` is not a file in the pipeline's folder: {}",
                product.name,
                full.display()
            )));
        }
        if let Some(inside) = root.as_deref().and_then(|root| {
            let full = std::fs::canonicalize(&full).ok()?;
            full.strip_prefix(root)
                .ok()
                .map(|inside| inside.to_string_lossy().replace('\\', "/"))
        }) {
            discovery.unmatched_files.retain(|file| *file != inside);
        }
        discovery
            .inventory
            .artifacts
            .push(SourceRecord::new(&product.name, EntityBinding::default()).at(relative));
    }
    Ok(())
}
