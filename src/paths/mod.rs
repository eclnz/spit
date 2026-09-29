//! Path rules: templates every step shares, the checks the pipeline compile
//! step makes of declared rules, and paths bound to resolved jobs.

mod bind;
mod rules;
mod template;

pub(crate) use self::bind::{bound_paths, case_collisions, output_keys};
pub use self::bind::{validate_concrete_paths, validate_source_files, VerifiedFiles};
pub(crate) use self::rules::{collect_paths, validate_discovery_rule};
pub use self::rules::{inspect_paths, PathCoverage, PathCoverageEntry, PathRule};
pub(crate) use self::template::{
    bind_path, decode_component, encode_component, error, PathPart, PathPlaceholder,
};
pub use self::template::{PathError, PathTemplate};
