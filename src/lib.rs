//! SPIT: resolve artifact pipelines, bind paths, and generate Bash scripts.

pub mod bash;
mod command;
mod compile;
pub mod diagnostics;
pub mod error;
mod imports;
pub mod inputs;
mod lower;
pub mod model;
pub mod parser;
pub mod paths;
pub mod render;
pub mod resolver;
mod shape;
mod span;
mod template;
pub mod types;

pub use bash::{render_bash, BashError};
pub use command::{validate_commands, CommandError, CommandTemplate};
pub use compile::validate_pipeline;
pub use diagnostics::{
    diagnose, diagnose_artifacts_at, diagnose_at, diagnose_at_with_inputs, Diagnostic,
    DiagnosticSource, Severity,
};
pub use error::{DefinitionSubject, ResolveError, TypeConflict};
pub use imports::{
    parse_document_at, parse_pipeline_at, parse_spit_at, parse_spit_without_records_at,
};
pub use inputs::{
    discover_source_files, discover_sources, parse_input_spec, parse_input_spec_at, Discovery,
    InputSource, InputSpec, ResolvedInputs,
};
pub use lower::{parse_document, parse_pipeline, parse_spit, Document};
pub use model::{
    natural_cmp, stage_and_parents, stage_within, ArtifactInstance, ArtifactKey, ArtifactReport,
    ArtifactType, Cardinality, CommandDef, CommandRole, CountRequirement, CoverageAction,
    CoverageGap, CoverageRule, DefaultPort, DirectoryDiscovery, EntityBinding, Gap, IncompleteJob,
    InputBinding, InputPort, InputRules, Invocation, Job, OperationDef, OutputPort, Pipeline,
    ProductDef, ResolvedDag, ShapeRule, SourceInventory, SourceRecord, StageDef, DEFAULT_OUTPUT,
};
pub use parser::{parse_source_inventory, render_source_inventory, ParseError, ParseErrorKind};
pub use paths::{
    inspect_paths, validate_concrete_paths, validate_source_files, PathCoverage, PathCoverageEntry,
    PathError, PathRule, PathTemplate, VerifiedFiles,
};
pub use render::{render_artifacts, render_bound_dag, render_dag, render_dag_json};
pub use resolver::{resolve, resolve_artifacts, resolve_artifacts_excluding};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
