//! Path rules: the checks the pipeline compile step makes of declared rules,
//! and paths bound to resolved jobs. The templates themselves are in
//! `crate::template`.

mod bind;
mod components;
mod product;
mod rules;

pub(crate) use self::bind::{bound_paths, case_collisions, check_rules};
pub use self::bind::{
    validate_bound_source_files, validate_source_files, BoundPaths, VerifiedFiles,
};
pub(crate) use self::components::validate_context_pattern;
pub(crate) use self::product::{shown_path, unspelled_entities, PathBinder};
pub(crate) use self::rules::collect_paths;
pub use self::rules::{inspect_paths, PathCoverage, PathCoverageEntry, PathRule};
