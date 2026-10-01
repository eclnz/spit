//! Path rules: templates every step shares, the checks the pipeline compile
//! step makes of declared rules, and paths bound to resolved jobs.

mod bind;
mod rules;
mod template;

pub(crate) use self::bind::{bound_paths, case_collisions, check_rules};
pub use self::bind::{
    validate_bound_source_files, validate_source_files, BoundPaths, VerifiedFiles,
};
pub(crate) use self::rules::collect_paths;
pub use self::rules::{inspect_paths, PathCoverage, PathCoverageEntry, PathRule};
pub(crate) use self::template::{
    decode_component, encode_component, error, require_directory, shown_path,
    validate_discovery_rule, PathBinder, PathPart, PathPlaceholder,
};
pub use self::template::{PathError, PathProblem, PathTemplate};
