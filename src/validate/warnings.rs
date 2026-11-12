//! Legal but likely mistaken pipeline text, found without an inventory.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::lower::ParsedDocument;
use crate::model::{stage_within, CommandRole, InputBinding};

/// A pipeline that checks but is probably not what its author meant. Each
/// variant names what it is about; the diagnostics place it in the text.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PipelineWarning<'a> {
    /// A stage that holds no steps, directly or in the stages nested in it.
    EmptyStage { stage: &'a str },
    /// An operation declared in a stage but called outside it. A stage does
    /// not limit where its operations are used, so this is legal.
    OperationOutsideStage {
        operation: &'a str,
        /// The stage it is declared in.
        declared: &'a str,
        /// The stage of its first call outside `declared`, `None` for the
        /// top level.
        called: Option<&'a str>,
        /// The line of that call, when the step has one.
        line: Option<usize>,
        /// The innermost stage holding the declaration and every call,
        /// `None` for the top level.
        common: Option<&'a str>,
    },
    /// A product named after the operation that makes it, as in
    /// `digest = digest(log)`.
    ProductNamedAfterOperation { product: &'a str },
    /// A source product that no step reads.
    UnusedSource { product: &'a str },
    /// An operation that no step uses.
    UnusedOperation { operation: &'a str },
    /// An operation that steps use but that has no command, in a pipeline
    /// where other operations have one.
    NoCommand { operation: &'a str },
    /// An output type variable of an operation that none of its inputs binds.
    UnboundOutputVariable {
        operation: &'a str,
        variable: String,
    },
}

impl fmt::Display for PipelineWarning<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyStage { stage } => write!(f, "stage `{stage}` has no steps"),
            Self::OperationOutsideStage {
                operation,
                declared,
                called,
                line,
                common,
            } => {
                write!(f, "operation `{operation}` is declared in stage `{declared}` but called ")?;
                match called {
                    Some(stage) => write!(f, "in stage `{stage}`")?,
                    None => f.write_str("outside every stage")?,
                }
                if let Some(line) = line {
                    write!(f, " on line {line}")?;
                }
                f.write_str("; a stage does not limit where its operations are used, so declare it in ")?;
                match common {
                    Some(stage) => write!(f, "stage `{stage}`, which holds every call"),
                    None => f.write_str("the top level, outside every stage"),
                }
            }
            Self::ProductNamedAfterOperation { product } => write!(
                f,
                "product `{product}` has the name of the operation that makes it; \
                 name the result instead, so the step reads as what it makes"
            ),
            Self::UnusedSource { product } => {
                write!(f, "source product `{product}` is never used as an input")
            }
            Self::UnusedOperation { operation } => {
                write!(f, "operation `{operation}` is declared but never used")
            }
            Self::NoCommand { operation } => {
                write!(f, "operation `{operation}` has no command, so its jobs cannot run")
            }
            Self::UnboundOutputVariable { operation, variable } => write!(
                f,
                "output type variable `{variable}` of `{operation}` appears in no input; it is known only where the output product declares its type"
            ),
        }
    }
}

/// The warnings of `document`, in the order they are reported. Names in
/// `skip` already have an error. Nothing is reported as unused in a file with
/// no steps, which is a library of definitions, nor for imported names: a
/// library is imported for the definitions a pipeline needs, and is linted on
/// its own.
pub(crate) fn collect_warnings<'a>(
    document: &'a ParsedDocument,
    skip: &BTreeSet<String>,
) -> Vec<PipelineWarning<'a>> {
    // A library, with no steps, may declare what it never uses.
    if document.pipeline.invocations.is_empty() {
        return operation_warnings(document, skip, true);
    }
    let mut warnings = stage_warnings(document);
    warnings.extend(placement_warnings(document));
    warnings.extend(name_warnings(document));
    warnings.extend(product_warnings(document, skip));
    warnings.extend(operation_warnings(document, skip, false));
    warnings
}

fn stage_warnings(document: &ParsedDocument) -> Vec<PipelineWarning<'_>> {
    let pipeline = &document.pipeline;
    pipeline
        .stages
        .iter()
        .filter(|stage| {
            !pipeline.invocations.iter().any(|invocation| {
                invocation
                    .stage
                    .as_deref()
                    .is_some_and(|name| stage_within(name, &stage.name))
            })
        })
        .map(|stage| PipelineWarning::EmptyStage { stage: &stage.name })
        .collect()
}

fn placement_warnings(document: &ParsedDocument) -> Vec<PipelineWarning<'_>> {
    let lines = &document.lines;
    /// The first call outside the declaring stage, and the innermost stage
    /// holding the declaration and every call, `None` for the top level.
    struct Outside<'a> {
        declared: &'a str,
        line: Option<usize>,
        stage: Option<&'a str>,
        common: Option<&'a str>,
    }
    let mut outside: BTreeMap<&str, Outside> = BTreeMap::new();
    for invocation in &document.pipeline.invocations {
        let operation = invocation.operation.as_str();
        let Some(declared) = lines.operation_stages.get(operation) else {
            continue;
        };
        let called = invocation.stage.as_deref();
        if called.is_some_and(|stage| stage_within(stage, declared)) {
            continue;
        }
        let entry = outside.entry(operation).or_insert_with(|| Outside {
            declared,
            line: invocation
                .outputs
                .first()
                .and_then(|output| lines.invocations.get(output))
                .map(|step| step.line),
            stage: called,
            common: Some(declared),
        });
        entry.common = entry
            .common
            .zip(called)
            .and_then(|(common, called)| common_stage(common, called));
    }
    outside
        .into_iter()
        .map(
            |(operation, found)| PipelineWarning::OperationOutsideStage {
                operation,
                declared: found.declared,
                called: found.stage,
                line: found.line,
                common: found.common,
            },
        )
        .collect()
}

/// The innermost stage holding both `first` and `second`, if any.
fn common_stage<'a>(first: &'a str, second: &str) -> Option<&'a str> {
    let mut end = None;
    for (index, _) in first.match_indices('/').chain([(first.len(), "")]) {
        if stage_within(second, &first[..index]) {
            end = Some(index);
        } else {
            break;
        }
    }
    end.map(|end| &first[..end])
}

fn name_warnings(document: &ParsedDocument) -> Vec<PipelineWarning<'_>> {
    document
        .pipeline
        .invocations
        .iter()
        .flat_map(|invocation| {
            invocation
                .outputs
                .iter()
                .filter(move |output| **output == invocation.operation)
        })
        .map(|product| PipelineWarning::ProductNamedAfterOperation { product })
        .collect()
}

fn product_warnings<'a>(
    document: &'a ParsedDocument,
    skip: &BTreeSet<String>,
) -> Vec<PipelineWarning<'a>> {
    let pipeline = &document.pipeline;
    let used: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| {
            invocation
                .outputs
                .iter()
                .map(String::as_str)
                .chain(invocation.inputs.iter().map(InputBinding::product_name))
        })
        .collect();
    pipeline
        .products
        .iter()
        .map(|product| product.name.as_str())
        .filter(|name| !skip.contains(*name) && !document.lines.imported.contains(*name))
        .filter(|name| !used.contains(name))
        .map(|product| PipelineWarning::UnusedSource { product })
        .collect()
}

fn operation_warnings<'a>(
    document: &'a ParsedDocument,
    skip: &BTreeSet<String>,
    library: bool,
) -> Vec<PipelineWarning<'a>> {
    let pipeline = &document.pipeline;
    // A call to an operation with a body leaves the body's steps, so the
    // call itself is in `calls`.
    let used: BTreeSet<_> = pipeline
        .invocations
        .iter()
        .map(|invocation| invocation.operation.as_str())
        .chain(pipeline.calls.iter().map(|call| call.operation.as_str()))
        .collect();
    let commands: BTreeSet<_> = pipeline
        .commands
        .iter()
        .filter(|command| command.role == CommandRole::Run)
        .map(|command| command.operation.as_str())
        .collect();
    let mut warnings = Vec::new();
    for operation in &pipeline.operations {
        let name = operation.name.as_str();
        if skip.contains(name) {
            continue;
        }
        let imported = document.lines.imported.contains(name);
        if !used.contains(name) {
            if !imported && !library {
                warnings.push(PipelineWarning::UnusedOperation { operation: name });
            }
        } else if !pipeline.commands.is_empty()
            && !commands.contains(name)
            && operation.steps.is_empty()
        {
            // Only once commands are in use: a pipeline may be written for its DAG alone.
            warnings.push(PipelineWarning::NoCommand { operation: name });
        }
        if imported {
            continue;
        }
        let bound: BTreeSet<_> = operation
            .inputs
            .iter()
            .flat_map(|port| port.artifact_type.variables())
            .collect();
        let produced: BTreeSet<_> = operation
            .outputs
            .iter()
            .flat_map(|port| port.artifact_type.variables())
            .collect();
        warnings.extend(produced.difference(&bound).map(|variable| {
            PipelineWarning::UnboundOutputVariable {
                operation: name,
                variable: variable.clone(),
            }
        }));
    }
    warnings
}
