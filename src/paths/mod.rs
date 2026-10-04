//! Path rules: templates every step shares, the checks the pipeline compile
//! step makes of declared rules, and paths bound to resolved jobs.

mod bind;
mod components;
mod product;
mod rules;
mod shape;
mod template;

pub(crate) use self::bind::{bound_paths, case_collisions, check_rules, dashed_labels};
pub use self::bind::{
    validate_bound_source_files, validate_source_files, BoundPaths, VerifiedFiles,
};
pub(crate) use self::components::{decode_component, encode_component, validate_discovery_rule};
pub(crate) use self::product::{shown_path, stage_directories, PathBinder};
pub(crate) use self::rules::collect_paths;
pub use self::rules::{inspect_paths, PathCoverage, PathCoverageEntry, PathRule};
pub(crate) use self::shape::{is_date, is_year, Shape};
pub(crate) use self::template::{
    error, product_text, require_directory, Holder, PathPart, PathPlaceholder,
};
pub use self::template::{PathError, PathProblem, PathTemplate};
