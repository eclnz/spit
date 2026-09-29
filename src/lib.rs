//! SPIT: compile artifact pipelines, settle their inputs, and resolve and
//! bind their jobs into a `.spitdag`.

mod command;
mod compile;
mod diagnostics;
mod error;
mod imports;
mod inputs;
mod json;
mod lower;
mod model;
mod parser;
mod paths;
mod render;
mod resolver;
mod shape;
mod span;
mod spitdag;
mod template;
mod types;

pub use command::{validate_commands, CommandError, CommandTemplate};
pub use compile::validate_pipeline;
pub use diagnostics::{
    diagnose, diagnose_artifacts_at, diagnose_at, diagnose_at_checked, diagnose_at_with_inputs,
    diagnose_recipe, diagnose_recipe_against, render_diagnostics_json, Diagnosis, Diagnostic,
    DiagnosticSource, Severity,
};
pub use error::{DefinitionSubject, ResolveError, TypeConflict};
pub use imports::parse_pipeline_at;
pub use inputs::{
    discover_source_files, discover_sources, parse_input_spec, parse_input_spec_at, Discovery,
    InputSource, InputSpec, ResolvedInputs,
};
pub use lower::parse_pipeline;
pub use model::{
    stage_within, ArtifactInstance, ArtifactKey, ArtifactReport, ArtifactType, Cardinality,
    CommandDef, CommandRole, CountRequirement, CoverageAction, CoverageGap, CoverageRule,
    DefaultPort, DirectoryDiscovery, EntityBinding, Gap, IncompleteJob, InputBinding, InputPort,
    InputRules, Invocation, Job, OperationDef, OutputPort, Pipeline, ProductDef, ResolvedDag,
    ShapeRule, SourceInventory, SourceRecord, StageDef,
};
pub use parser::{parse_source_inventory, render_source_inventory, ParseError, ParseErrorKind};
pub use paths::{
    inspect_paths, validate_source_files, PathCoverage, PathCoverageEntry, PathError, PathRule,
    PathTemplate, VerifiedFiles,
};
pub use render::{render_artifacts, render_dag};
pub use resolver::{bind_dag, resolve, resolve_artifacts_excluding};
pub use spitdag::{render_bound_dag, ArgPart, Argument, BoundArtifact, BoundDag, BoundJob};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
