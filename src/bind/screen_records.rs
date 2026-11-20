use crate::model::{CheckKind, PipelineIndex};
use crate::spitdag::BoundDag;

use super::BindError;

/// Bind record paths once per product and check them against artifact paths.
pub(super) fn bind_screen_records(
    dag: &mut BoundDag,
    index: &PipelineIndex<'_>,
) -> Result<(), BindError> {
    if !index
        .pipeline
        .checks
        .iter()
        .any(|check| check.kind == CheckKind::Screen)
    {
        return Ok(());
    }
    let products: std::collections::BTreeSet<_> = dag
        .jobs
        .iter()
        .flat_map(|job| {
            job.checks
                .iter()
                .filter(|check| dag.step(job).checks[check.check].kind == CheckKind::Screen)
                .map(|check| dag.artifact(check.artifact).product.to_owned())
        })
        .collect();
    let mut occupied: std::collections::BTreeSet<_> = dag
        .jobs
        .iter()
        .flat_map(|job| job.input_artifacts().chain(job.outputs.iter().copied()))
        .map(|artifact| dag.path(artifact).to_lowercase())
        .collect();
    for product in products {
        let path = crate::paths::screen_record_path(index, &product);
        let folded = path.to_lowercase();
        let prefix = format!("{folded}/");
        let overlaps = folded
            .match_indices('/')
            .any(|(end, _)| occupied.contains(&folded[..end]))
            || occupied
                .range(prefix.clone()..)
                .next()
                .is_some_and(|other| other.starts_with(&prefix));
        if overlaps || !occupied.insert(folded) {
            return Err(crate::template::error(format!(
                "screen record for `{product}` collides at `{path}`"
            ))
            .into());
        }
        dag.screen_records.insert(product, path);
    }
    Ok(())
}
