//! Checks of `resources` lines against the operations they name: each names
//! an operation that runs a command, once.

use std::collections::{BTreeMap, BTreeSet};

use crate::command::CommandError;
use crate::model::{OperationDef, Pipeline};
use crate::parser::SourceMap;

/// Every error in `pipeline`'s `resources` lines, given its operations by
/// name. Lines for operations in `skip` belong to declarations that already
/// failed and are not checked.
pub(crate) fn collect_resources(
    pipeline: &Pipeline,
    operations: &BTreeMap<&str, &OperationDef>,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<CommandError> {
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, resources) in pipeline.resources.iter().enumerate() {
        let name = resources.operation.as_str();
        if skip.contains(name) {
            continue;
        }
        let message = match operations.get(name) {
            None => format!("`resources` refers to unknown operation `{name}`"),
            Some(operation) if !operation.steps.is_empty() => format!(
                "operation `{name}` is carried out by the steps in its body, so it takes no `resources` line; give each step's operation its own"
            ),
            Some(_) if !seen.insert(name) => {
                format!("operation `{name}` has a second `resources` line; give its cores and memory on one")
            }
            Some(_) => continue,
        };
        errors.push(
            CommandError::new(message)
                .at(lines.resources.get(index).cloned())
                .focus(name),
        );
    }
    errors
}
