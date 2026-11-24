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

pub(crate) struct PipelineCheck<'a> {
    pub(crate) definitions: DefinitionCheck<'a>,
    pub(crate) rules: Vec<(DefinitionSubject, ResolveError)>,
    pub(crate) commands: Vec<CommandError>,
    pub(crate) paths: Vec<PathError>,
    pub(crate) warnings: Vec<PipelineWarning<'a>>,
}

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
