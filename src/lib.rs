//! SPIT: compile artifact pipelines, settle their inputs, and resolve and
//! bind their jobs into a `.spitdag`.

// Outside tests, `expect` states the invariant it relies on; see AGENTS.md.
#![deny(clippy::unwrap_used)]

mod bind;
mod blob;
mod builtins;
mod check;
mod command;
mod compile;
mod diagnostics;
mod editor;
mod inputs;
mod json;
mod lower;
mod model;
mod order;
mod parser;
mod paths;
mod render;
mod resolver;
mod resources;
mod review;
mod shape;
mod span;
mod spitdag;
mod template;
mod types;
mod validate;

pub use bind::{bind_dag, bind_dag_with, BindError};
pub use builtins::{builtin_words, Doc, Word, WordUse, DOCS, REFERENCE};
pub use compile::{validate_commands, validate_pipeline};
pub use diagnostics::{
    diagnose, diagnose_checked, diagnose_checked_with_inventory, diagnose_checked_with_records,
    diagnose_in, diagnose_inputs, diagnose_recipe, diagnose_recipe_against,
    render_calls_check_json, render_check_json, render_diagnostics_json, Checked, Context,
    Diagnosis, Diagnostic, DiagnosticSource, FileNames, Records, Related, Severity, ShownPath,
    SourceLines,
};
pub use editor::{
    pipeline_hovers, render_editor_json, render_recipe_editor_json, render_words_json, Hover,
    HoverKind,
};
pub use inputs::{
    discover_source_files, discover_sources, parse_input_spec, parse_input_spec_at, variants,
    Discovery, EveryGroupDropped, InputError, InputSource, InputSpec, Likeness, MissedSource,
    NearestFile, ResolvedInputs, ReviewFileError, Spelling, UnmatchedExclusion, Variants,
};
pub use lower::imports::parse_pipeline_at;
pub use lower::parse_pipeline;
pub use model::{
    stage_within, Artifact, ArtifactId, ArtifactInstance, ArtifactKey, ArtifactReport,
    ArtifactType, Artifacts, Beside, BesideFile, BodyStep, Call, CallId, Cardinality, CommandDef,
    CommandRole, ContextPattern, CountRequirement, CoverageAction, CoverageGap, CoverageRule,
    DagStep, DefaultReviews, DefinitionSubject, DimensionOrders, EntityBinding, Exclusion, Gap,
    IncompleteJob, InputBinding, InputPort, InputRules, Invocation, Job, JobId, Judged, NearMiss,
    OperationDef, OutputPort, Pipeline, Port, PortSite, ProductDef, Removal, ResolveError,
    ResolvedDag, Resources, ResourcesDef, ReviewDef, ReviewFile, ShapeRule, SidecarGroup,
    SourceFile, SourceInventory, SourceRecord, StageDef, StepId, StepOrigin, StepOutput,
    TypeChecking, TypeConflict, Under, UnmatchedVerdicts, Verdict, VerdictRow, Withheld,
};
pub use parser::{parse_source_inventory, render_source_inventory, ParseError, ParseErrorKind};
pub use paths::{
    inspect_paths, validate_bound_source_files, validate_source_files, BoundPaths, PathCoverage,
    PathCoverageEntry, PathRule, VerifiedFiles,
};
pub use render::{
    render_artifacts, render_artifacts_by_target, render_bound_dag, render_call, render_calls,
    render_calls_json, render_dag, render_pipeline_tree, render_step_counts,
    unused_sources_summary, View,
};
pub use resolver::{resolve, resolve_artifacts_excluding, resolve_artifacts_partial};
pub use spitdag::{
    ArgPart, Argument, BoundArtifact, BoundCall, BoundCheck, BoundCommand, BoundDag, BoundJob,
    BoundStep, LeftOut, StepCall, StepCheck, When,
};
pub use template::{
    CommandError, CommandProblem, CommandTemplate, EntitiesFormat, NamedFile, PathError,
    PathProblem, PathTemplate,
};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
