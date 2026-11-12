//! Every check of a parsed pipeline and its input rules, with no inventory,
//! in one fixed order. The diagnostics place and render what this finds.

mod warnings;

pub(crate) use warnings::PipelineWarning;

use crate::compile::{
    collect_command_lines, collect_definitions, operations_by_name, DefinitionCheck,
};
use crate::inputs::collect_rule_errors;
use crate::lower::ParsedDocument;
use crate::model::{DefinitionSubject, ResolveError};
use crate::paths::collect_paths;
use crate::template::CommandError;
use crate::template::PathError;

/// Every error and warning [`check_pipeline`] finds, by the check that
/// found it.
pub(crate) struct PipelineCheck<'a> {
    /// The declarations and steps: the steps that checked, their errors, and
    /// the names that failed or depend on a failure. Every later check skips
    /// what `definitions.poisoned` names rather than report it again.
    pub(crate) definitions: DefinitionCheck<'a>,
    /// The input rules' errors, checked against the pipeline's sources.
    pub(crate) rules: Vec<(DefinitionSubject, ResolveError)>,
    /// The errors of the `command`, `resources`, `check` and `review` lines,
    /// in that order.
    pub(crate) commands: Vec<CommandError>,
    /// The path rules' errors.
    pub(crate) paths: Vec<PathError>,
    /// What checks but is likely a mistake, about names that did not fail.
    pub(crate) warnings: Vec<PipelineWarning<'a>>,
}

/// Check `document` without an inventory: definitions, input rules,
/// commands, resources, checks, reviews and paths, in that order, which is
/// the order the diagnostics list their errors in, then warnings.
pub(crate) fn check_pipeline(document: &ParsedDocument) -> PipelineCheck<'_> {
    let (pipeline, lines) = (&document.pipeline, &document.lines);
    let definitions = collect_definitions(pipeline);
    let skip = &definitions.poisoned;
    let rules = collect_rule_errors(pipeline, &document.inputs, skip);
    let operations = operations_by_name(pipeline);
    let commands = collect_command_lines(pipeline, &operations, lines, skip);
    let paths = collect_paths(pipeline, lines, skip).1;
    let warnings = warnings::collect_warnings(document, skip);
    PipelineCheck {
        definitions,
        rules,
        commands,
        paths,
        warnings,
    }
}
