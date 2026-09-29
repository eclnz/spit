//! SPIT: resolve artifact pipelines, bind paths, and generate Bash scripts.

pub mod bash;
mod command;
pub mod diagnostics;
pub mod error;
mod imports;
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
pub use diagnostics::{
    diagnose, diagnose_artifacts_at, diagnose_at, Diagnostic, DiagnosticSource, Severity,
};
pub use error::{DefinitionSubject, ResolveError, TypeConflict};
pub use imports::{parse_document_at, parse_pipeline_at};
pub use lower::{parse_document, parse_pipeline};
pub use model::{
    natural_cmp, stage_and_parents, stage_within, ArtifactInstance, ArtifactKey, ArtifactReport,
    ArtifactType, Cardinality, CommandDef, CommandRole, CountRequirement, CoverageAction,
    CoverageGap, CoverageRule, DefaultPort, DirectoryDiscovery, EntityBinding, Gap, IncompleteJob,
    InputBinding, InputPort, Invocation, Job, OperationDef, OutputPort, Pipeline, ProductDef,
    ResolvedDag, ShapeRule, SourceInventory, SourceRecord, StageDef, DEFAULT_OUTPUT,
};
pub use parser::{parse_source_inventory, render_source_inventory, ParseError, ParseErrorKind};
pub use paths::{
    discover_source_files, discover_sources, inspect_paths, validate_concrete_paths,
    validate_source_files, PathCoverage, PathCoverageEntry, PathError, PathRule, PathTemplate,
    VerifiedFiles,
};
pub use render::{render_artifacts, render_bound_dag, render_dag, render_dag_json};
pub use resolver::{resolve, resolve_artifacts, validate_pipeline};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
