//! SPIT: resolve artifact pipelines, bind paths, and generate Bash scripts.

pub mod bash;
pub mod diagnostics;
pub mod error;
mod imports;
pub mod model;
pub mod parser;
pub mod paths;
pub mod render;
pub mod resolver;
mod span;
mod template;
pub mod types;

pub use bash::{render_bash, validate_commands, BashError};
pub use diagnostics::{
    diagnose, diagnose_artifacts_at, diagnose_at, Diagnostic, DiagnosticSource, Severity,
};
pub use error::{DefinitionSubject, ResolveError, TypeConflict};
pub use imports::{parse_document_at, parse_pipeline_at};
pub use model::{
    natural_cmp, stage_and_parents, stage_within, ArtifactInstance, ArtifactKey, ArtifactReport,
    ArtifactType, Cardinality, CommandDef, CommandRole, CountRequirement, CoverageGap,
    CoverageRule, EntityBinding, Gap, IncompleteJob, InputBinding, InputPort, Invocation, Job,
    OperationDef, OutputPort, Pipeline, ProductDef, ResolvedDag, ShapeRule, SourceInventory,
    SourceRecord, StageDef, DEFAULT_OUTPUT,
};
pub use parser::{
    parse_document, parse_pipeline, parse_source_inventory, render_source_inventory, ParseError,
    ParseErrorKind,
};
pub use paths::{
    discover_sources, inspect_paths, validate_concrete_paths, validate_source_files, PathCoverage,
    PathCoverageEntry, PathError, PathRule, VerifiedFiles,
};
pub use render::{render_artifacts, render_bound_dag, render_dag};
pub use resolver::{resolve, resolve_artifacts, validate_pipeline};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
