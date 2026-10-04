//! SPIT: compile artifact pipelines, settle their inputs, and resolve and
//! bind their jobs into a `.spitdag`.

// Outside tests, `expect` states the invariant it relies on; see AGENTS.md.
#![deny(clippy::unwrap_used)]

mod blob;
mod builtins;
mod check;
mod command;
mod compile;
mod diagnostics;
mod editor;
mod error;
mod imports;
mod inputs;
mod json;
mod lower;
mod model;
mod order;
mod parser;
mod paths;
mod render;
mod resolver;
mod shape;
mod span;
mod spitdag;
mod template;
mod types;

pub use builtins::{builtin_words, Doc, Word, WordUse, DOCS, REFERENCE};
pub use command::{validate_commands, CommandError, CommandProblem, CommandTemplate};
pub use compile::validate_pipeline;
pub use diagnostics::{
    diagnose, diagnose_checked, diagnose_checked_with_inventory, diagnose_checked_with_records,
    diagnose_in, diagnose_inputs, diagnose_recipe, diagnose_recipe_against, render_check_json,
    render_diagnostics_json, Checked, Context, Diagnosis, Diagnostic, DiagnosticSource, FileNames,
    Records, Related, Severity, ShownPath,
};
pub use editor::{pipeline_hovers, render_editor_json, render_words_json, Hover, HoverKind};
pub use error::{DefinitionSubject, NearMiss, PortSite, ResolveError, TypeConflict};
pub use imports::parse_pipeline_at;
pub use inputs::{
    case_variants, discover_source_files, discover_sources, parse_input_spec, parse_input_spec_at,
    CaseVariants, Discovery, EveryGroupDropped, InputError, InputSource, InputSpec, MissedSource,
    NearestFile, NearlyMatched, ResolvedInputs, Spelling, SuggestedSource, Suggestions,
    UnmatchedExclusion,
};
pub use lower::parse_pipeline;
pub use model::{
    stage_within, Artifact, ArtifactId, ArtifactInstance, ArtifactKey, ArtifactReport,
    ArtifactType, Artifacts, Beside, BodyStep, Call, CallId, Cardinality, CommandDef, CommandRole,
    CountRequirement, CoverageAction, CoverageGap, CoverageRule, DagStep, DimensionOrders,
    DirectoryDiscovery, EntityBinding, Exclusion, Gap, IncompleteJob, InputBinding, InputPort,
    InputRules, Invocation, Job, JobId, OperationDef, OutputPort, Pipeline, Port, ProductDef,
    Removal, ResolvedDag, ShapeRule, SidecarGroup, SourceFile, SourceInventory, SourceRecord,
    StageDef, StepId, StepOrigin, StepOutput,
};
pub use parser::{parse_source_inventory, render_source_inventory, ParseError, ParseErrorKind};
pub use paths::{
    inspect_paths, validate_bound_source_files, validate_source_files, BoundPaths, PathCoverage,
    PathCoverageEntry, PathError, PathProblem, PathRule, PathTemplate, VerifiedFiles,
};
pub use render::{
    render_artifacts, render_artifacts_by_target, render_bound_dag, render_call, render_dag,
    render_step_counts, unused_sources_summary, View,
};
pub use resolver::{
    bind_dag, bind_dag_with, resolve, resolve_artifacts_excluding, resolve_artifacts_partial,
    BindError,
};
pub use spitdag::{
    ArgPart, Argument, BoundArtifact, BoundCall, BoundCheck, BoundDag, BoundJob, BoundStep,
    LeftOut, StepCall, StepCheck, When,
};
pub use types::{
    parse_type_expr, Compatibility, Substitutions, TypeExpr, TypeParseError, TypeUnifyError,
};
