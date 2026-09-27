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
pub use diagnostics::{diagnose, diagnose_at, Diagnostic, DiagnosticSource, Severity};
pub use error::{DefinitionSubject, ResolveError};
pub use imports::parse_document_at;
pub use model::{
    ArtifactInstance, ArtifactKey, ArtifactType, Cardinality, CommandDef, CountRequirement,
    CoverageRule, EntityBinding, InputBinding, InputPort, Invocation, Job, OperationDef, Pipeline,
    ProductDef, ResolvedDag, ShapeRule, SourceInventory, SourceRecord,
};
pub use parser::{
    parse_document, parse_pipeline, parse_source_inventory, ParseError, ParseErrorKind,
};
pub use paths::{
    inspect_paths, validate_concrete_paths, validate_source_files, PathCoverage, PathCoverageEntry,
    PathError, PathRule,
};
pub use render::{render_bound_dag, render_dag};
pub use resolver::{resolve, validate_pipeline};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
