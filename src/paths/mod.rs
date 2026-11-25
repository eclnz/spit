mod bind;
mod components;
mod product;
mod rules;

pub(crate) use self::bind::{
    bound_paths, case_collisions, check_rules, misread_sources, HERE_WITHOUT_FILE,
};
pub use self::bind::{
    validate_bound_source_files, validate_source_files, BoundPaths, VerifiedFiles,
};
pub(crate) use self::components::{unusable_path, validate_context_pattern};
pub(crate) use self::product::{screen_record_path, shown_paths, unspelled_entities, PathBinder};
pub(crate) use self::rules::collect_paths;
pub use self::rules::{inspect_paths, PathCoverage, PathCoverageEntry, PathRule};
