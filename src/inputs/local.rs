//! Fixed pipeline-local sources use their declaring folder, independently of the dataset.

use std::path::Path;

use crate::model::{ArtifactInstance, EntityBinding, Pipeline, SourceInventory, SourceRecord};
use crate::paths::PathBinder;
use crate::template::{error, PathError};

/// Add each fixed local source, retaining its path relative to the pipeline.
pub(super) fn append(
    pipeline: &Pipeline,
    inventory: &mut SourceInventory,
) -> Result<(), PathError> {
    let mut binder = PathBinder::new(pipeline);
    for product in &pipeline.products {
        let Some(folder) = &product.here else {
            continue;
        };
        let root = pipeline
            .folder
            .as_deref()
            .ok_or_else(|| error("a here source needs a pipeline read from a file"))?;
        let artifact = ArtifactInstance::new(
            &product.name,
            product.artifact_type.clone(),
            EntityBinding::default(),
        );
        let relative = binder.bind(artifact.view(), || {
            format!("local source `{}`", product.name)
        })?;
        let relative = if folder.is_empty() {
            relative
        } else {
            format!("{folder}/{relative}")
        };
        let full = Path::new(root).join(&relative);
        if !full.is_file() {
            return Err(error(format!(
                "missing pipeline-local source `{}`: {}",
                product.name,
                full.display()
            )));
        }
        inventory
            .artifacts
            .push(SourceRecord::new(&product.name, EntityBinding::default()).at(relative));
    }
    Ok(())
}
