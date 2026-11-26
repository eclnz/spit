use std::path::{Path, PathBuf};

use crate::model::{DefinitionSubject, InputRules, Pipeline, PipelineIndex, ResolveError};
use crate::review::ReviewIndex;

pub(crate) fn without_review_files(
    unmatched: Vec<String>,
    root: Option<&Path>,
    rules: &InputRules,
) -> Vec<String> {
    let reviews: Vec<PathBuf> = rules
        .reviews
        .iter()
        .filter_map(|review| std::fs::canonicalize(&review.path).ok())
        .collect();
    let Some(root) = root.filter(|_| !reviews.is_empty()) else {
        return unmatched;
    };
    unmatched
        .into_iter()
        .filter(|file| {
            std::fs::canonicalize(root.join(file)).map_or(true, |path| !reviews.contains(&path))
        })
        .collect()
}

pub(crate) fn collect_review_errors(
    pipeline: &Pipeline,
    rules: &InputRules,
) -> Vec<(DefinitionSubject, ResolveError)> {
    if rules.reviews.is_empty() {
        return Vec::new();
    }
    let index = PipelineIndex::new(pipeline);
    let reviewed = ReviewIndex::new(&index);
    rules
        .reviews
        .iter()
        .enumerate()
        .filter(|(_, file)| {
            !reviewed.of(&file.review).contains(&file.review.as_str())
                && !pipeline
                    .reviews
                    .iter()
                    .any(|known| known.name == file.review)
        })
        .map(|(index, file)| {
            let subject = DefinitionSubject::ReviewFile(index);
            let problem = ResolveError::InvalidDefinition {
                subject: subject.clone(),
                detail: format!(
                    "product `{}` has no review; add `@review` after the call that makes it",
                    file.review
                ),
            };
            (subject, problem)
        })
        .collect()
}
