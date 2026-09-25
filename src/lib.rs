//! SPIT: a small static compiler for logical artifact pipelines.

pub mod bash;
pub mod error;
pub mod model;
pub mod parser;
pub mod render;
pub mod resolver;
pub mod types;

pub use bash::{
    inspect_paths, render_bash, render_bound_dag, validate_concrete_paths, BashError, PathCoverage,
    PathCoverageEntry, PathRule,
};
pub use error::ResolveError;
pub use model::{
    ArtifactInstance, ArtifactType, Cardinality, CommandDef, CountRequirement, CoverageRule,
    EntityBinding, InputBinding, InputPort, Invocation, Job, OperationDef, Pipeline, ProductDef,
    ResolvedDag, ShapeRule, SourceInventory, SourceRecord,
};
pub use parser::{parse_document, parse_pipeline, parse_source_inventory, ParseError};
pub use render::render_dag;
pub use resolver::resolve;
pub use types::{parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeUnifyError};
