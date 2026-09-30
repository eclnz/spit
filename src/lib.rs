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

pub use command::{validate_commands, CommandError, CommandProblem, CommandTemplate};
pub use compile::validate_pipeline;
pub use diagnostics::{
    diagnose, diagnose_checked, diagnose_checked_with_inventory, diagnose_checked_with_records,
    diagnose_in, diagnose_recipe, diagnose_recipe_against, render_diagnostics_json, Checked,
    Context, Diagnosis, Diagnostic, DiagnosticSource, Records, Severity,
};
pub use error::{DefinitionSubject, PortSite, ResolveError, TypeConflict};
pub use imports::parse_pipeline_at;
pub use inputs::{
    discover_source_files, discover_sources, parse_input_spec, parse_input_spec_at, Discovery,
    InputError, InputSource, InputSpec, ResolvedInputs,
};
pub use lower::parse_pipeline;
pub use model::{
    stage_within, Artifact, ArtifactId, ArtifactInstance, ArtifactKey, ArtifactReport,
    ArtifactType, Artifacts, Cardinality, CommandDef, CommandRole, CountRequirement,
    CoverageAction, CoverageGap, CoverageRule, DirectoryDiscovery, EntityBinding, Gap,
    IncompleteJob, InputBinding, InputPort, InputRules, Invocation, Job, OperationDef, OutputPort,
    Pipeline, ProductDef, ResolvedDag, ShapeRule, SourceInventory, SourceRecord, StageDef,
};
pub use parser::{parse_source_inventory, render_source_inventory, ParseError, ParseErrorKind};
pub use paths::{
    inspect_paths, validate_bound_source_files, validate_source_files, BoundPaths, PathCoverage,
    PathCoverageEntry, PathError, PathProblem, PathRule, PathTemplate, VerifiedFiles,
};
pub use render::{render_artifacts, render_bound_dag, render_dag, View};
pub use resolver::{bind_dag, bind_dag_with, resolve, resolve_artifacts_excluding, BindError};
pub use spitdag::{ArgPart, Argument, BoundArtifact, BoundDag, BoundJob};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
