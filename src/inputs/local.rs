//! `here` sources: files kept beside the pipeline rather than in the
//! dataset, as a script a command runs is. Each is one file, found from the
//! pipeline's folder rather than read from the dataset.

use std::path::Path;

use crate::model::{ArtifactInstance, EntityBinding, Pipeline, SourceInventory, SourceRecord};
use crate::paths::{PathBinder, HERE_WITHOUT_FILE};
use crate::template::{error, PathError};

/// Add a record for each `here` source of `pipeline` to `inventory`, with
/// its path from the pipeline's folder. A file that is missing is an error.
pub(super) fn append(
    pipeline: &Pipeline,
    inventory: &mut SourceInventory,
) -> Result<(), PathError> {
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
        inventory
            .artifacts
            .push(SourceRecord::new(&product.name, EntityBinding::default()).at(relative));
    }
    Ok(())
}
