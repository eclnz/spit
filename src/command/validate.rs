//! Commands checked against their operations, and against the steps that
//! call them, without resolving jobs.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    Cardinality, CommandRole, Invocation, OperationDef, Pipeline, ProductDef, DEFAULT_OUTPUT,
};
use crate::parser::SourceMap;
use crate::template::Part;

use super::{CommandError, CommandTemplate, Stream};

/// What a placeholder in a command refers to.
pub(crate) enum Slot {
    Input(usize),
    Output(usize),
    /// `{@cores}`: the cores each job of the operation uses, from its
    /// `resources` line.
    Cores,
    /// Any other name: a dimension of the job, whose value the job's
    /// outputs carry. [`collect_commands`] checks that every step calling
    /// the operation has it.
    Dimension,
}

/// The placeholder a command gives the cores its job uses.
pub(crate) const CORES: &str = "@cores";

/// What a placeholder gives of its port's file: the path, or, for an output
/// as `{image.dir}` and `{image.stem}`, its folder or its file name without
/// its extension, for a tool that takes a folder and a name, or, for an
/// input as `{images.sub}`, each of its artifacts' value of a dimension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Facet<'a> {
    Path,
    Dir,
    Stem,
    Dimension(&'a str),
}

/// A placeholder's port and the facet of its file it gives. `.dir` and
/// `.stem` are always facets, so a dimension of either name cannot be read
/// this way.
pub(crate) fn facet(name: &str) -> (&str, Facet<'_>) {
    match name.split_once('.') {
        None => (name, Facet::Path),
        Some((port, "dir")) => (port, Facet::Dir),
        Some((port, "stem")) => (port, Facet::Stem),
        Some((port, dimension)) => (port, Facet::Dimension(dimension)),
    }
}

/// What `name` refers to in `operation`'s command. A port's name comes
/// before a dimension's, so a dimension that shares it cannot be written.
pub(crate) fn slot(operation: &OperationDef, name: &str) -> Option<Slot> {
    if name == "@output" {
        return operation
            .outputs
            .iter()
            .position(|port| port.name == DEFAULT_OUTPUT)
            .map(Slot::Output);
    }
    if name == CORES {
        return Some(Slot::Cores);
    }
    if name.starts_with('@') {
        return None;
    }
    if let Some(index) = operation.inputs.iter().position(|port| port.name == name) {
        return Some(Slot::Input(index));
    }
    let output = operation
        .outputs
        .iter()
        .position(|port| port.name == name && port.name != DEFAULT_OUTPUT);
    Some(output.map_or(Slot::Dimension, Slot::Output))
}

/// Check every declared command against its operation without resolving jobs:
/// the template must name only known placeholders and write every output; a
/// `verify` command may read inputs only. Checks are checked too.
pub fn validate_commands(pipeline: &Pipeline) -> Result<(), CommandError> {
    let lines = SourceMap::default();
    match collect_commands(pipeline, &lines, &BTreeSet::new())
        .into_iter()
        .next()
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Check every command, collecting each error, and then every `check` and
/// its uses. Commands for operations in `skip` belong to declarations that
/// already failed and are not checked.
pub(crate) fn collect_commands(
    pipeline: &Pipeline,
    lines: &SourceMap,
    skip: &BTreeSet<String>,
) -> Vec<CommandError> {
    let operations: BTreeMap<_, _> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation))
        .collect();
    let products: BTreeMap<_, _> = pipeline
        .products
        .iter()
        .map(|product| (product.name.as_str(), product))
        .collect();
    // The steps calling each operation, by the product each step's first
    // output makes, whose dimensions are its jobs'. A step that failed is
    // left out, as its products' dimensions may be wrong.
    let mut callers: BTreeMap<&str, Vec<Caller<'_>>> = BTreeMap::new();
    for invocation in &pipeline.invocations {
        if invocation
            .outputs
            .iter()
            .any(|output| skip.contains(output))
        {
            continue;
        }
        if let Some(&product) = invocation
            .outputs
            .first()
            .and_then(|output| products.get(output.as_str()))
        {
            callers
                .entry(invocation.operation.as_str())
                .or_default()
                .push(Caller {
                    invocation,
                    product,
                });
        }
    }
    let dimensions: BTreeSet<&str> = pipeline
        .products
        .iter()
        .flat_map(|product| product.dimensions.iter().map(String::as_str))
        .collect();
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, command) in pipeline.commands.iter().enumerate() {
        if skip.contains(&command.operation) {
            continue;
        }
        let line = lines.command(index);
        let Some(operation) = operations.get(command.operation.as_str()) else {
            errors.push(
                CommandError::new(format!(
                    "command refers to unknown operation `{}`",
                    command.operation
                ))
                .at(line)
                .focus(&command.operation),
            );
            continue;
        };
        if let Some(port) = operation.inputs.iter().find(|port| {
            operation
                .outputs
                .iter()
                .any(|output| output.name == port.name)
        }) {
            errors.push(
                CommandError::new(format!(
                    "operation `{}` has an input port named `{}`, which shadows `{{{}}}`",
                    operation.name, port.name, port.name
                ))
                .at(line.clone())
                .focus(&command.operation),
            );
        } else if command.role == CommandRole::Run && !seen.insert(command.operation.as_str()) {
            errors.push(
                CommandError::new(format!(
                    "duplicate command for operation `{}`",
                    command.operation
                ))
                .at(line)
                .focus(&command.operation),
            );
        } else if let Err(e) = check_command_placeholders(
            &command.template,
            operation,
            command.role,
            &Callers {
                steps: callers
                    .get(operation.name.as_str())
                    .map_or(&[], Vec::as_slice),
                products: &products,
                dimensions: &dimensions,
            },
        ) {
            errors.push(e.at(line));
        }
    }
    errors.extend(crate::resources::collect_resources(
        pipeline,
        &operations,
        lines,
        skip,
    ));
    errors.extend(crate::check::collect_checks(pipeline, lines, skip));
    errors
}

/// A step that calls an operation, and the product its first output makes.
struct Caller<'p> {
    invocation: &'p Invocation,
    product: &'p ProductDef,
}

/// What a command's dimension placeholders are checked against: the steps
/// that call its operation, the pipeline's products, which their inputs
/// read, and every dimension those products have.
struct Callers<'a, 'p> {
    steps: &'a [Caller<'p>],
    products: &'a BTreeMap<&'p str, &'p ProductDef>,
    dimensions: &'a BTreeSet<&'p str>,
}

fn check_command_placeholders(
    template: &CommandTemplate,
    operation: &OperationDef,
    role: CommandRole,
    callers: &Callers<'_, '_>,
) -> Result<(), CommandError> {
    let mut written = BTreeSet::new();
    let stdout = redirected_ports(template, operation, &mut written)?;
    for parts in template.arguments() {
        let whole = parts.len() == 1;
        for part in parts {
            let Part::Placeholder(name) = part else {
                continue;
            };
            let unknown = || {
                CommandError::new(format!(
                    "command for `{}` uses unknown placeholder `{{{name}}}`",
                    operation.name
                ))
                .focus(format!("{{{name}}}"))
            };
            let fail =
                |message: String| Err(CommandError::new(message).focus(format!("{{{name}}}")));
            let (port, facet) = facet(name);
            if port == DEFAULT_OUTPUT
                && operation
                    .outputs
                    .iter()
                    .any(|output| output.name == DEFAULT_OUTPUT)
            {
                return fail(
                    "`{output}` names no port; write `{@output}` for the output's path".to_owned(),
                );
            }
            let slot = slot(operation, port).ok_or_else(unknown)?;
            // An output's dimensions are its job's, written `{dimension}`.
            if let (Slot::Output(_), Facet::Dimension(other)) = (&slot, facet) {
                return fail(format!(
                    "`{{{name}}}` gives no `.{other}`; write `{{{port}}}` for the file, `{{{port}.dir}}` for its folder, or `{{{port}.stem}}` for its name without its extension"
                ));
            }
            match slot {
                Slot::Output(index) => {
                    if role == CommandRole::Verify {
                        return Err(CommandError::new(format!(
                            "verify for `{}` cannot use output `{{{name}}}`, which does not exist until the command runs",
                            operation.name
                        ))
                        .focus(format!("{{{name}}}")));
                    }
                    // A folder without an extension is its whole name.
                    let output = &operation.outputs[index];
                    if facet == Facet::Stem && output.extension.is_none() && !output.folder {
                        let example = if port == "@output" {
                            "-> Image .nii.gz".to_owned()
                        } else {
                            format!("{port}: Image .nii.gz")
                        };
                        let port_name = &operation.outputs[index].name;
                        return fail(format!(
                            "`{{{name}}}` is `{port_name}`'s file name without its extension, but `{}` declares none for `{port_name}`; give it one, as in `{example}`",
                            operation.name
                        ));
                    }
                    if stdout == Some(index) {
                        return fail(format!(
                            "`{{{name}}}`: `{port}` is the command's stdout, written `> {{{port}}}`, so it cannot be an argument too"
                        ));
                    }
                    // A folder and a name tell the tool where to write.
                    written.insert(index);
                }
                Slot::Cores if facet != Facet::Path => {
                    return fail(format!(
                        "`{{{name}}}`: `{{@cores}}` is a number, the cores the job uses, and gives no `.dir` or `.stem`"
                    ));
                }
                Slot::Cores => {}
                Slot::Dimension if facet != Facet::Path => {
                    return fail(format!(
                        "command for `{}` uses unknown placeholder `{{{name}}}`: `{}` has no port `{port}`",
                        operation.name, operation.name
                    ));
                }
                Slot::Dimension => check_dimension(operation, name, callers)?,
                Slot::Input(_) if matches!(facet, Facet::Dir | Facet::Stem) => {
                    return fail(format!(
                        "`{{{name}}}`: only an operation's outputs give `.dir` and `.stem`, for a tool that is told where to write"
                    ));
                }
                Slot::Input(index) => {
                    if let Facet::Dimension(dimension) = facet {
                        check_input_dimension(operation, index, name, dimension, callers)?;
                    }
                    if operation.inputs[index].cardinality == Cardinality::Many && !whole {
                        return Err(CommandError::new(format!(
                            "many input `{{{name}}}` must be a complete command argument"
                        ))
                        .focus(format!("{{{name}}}")));
                    }
                }
            }
        }
    }
    if role == CommandRole::Run {
        // A tool writes an output beside another without being told where.
        if let Some(port) = operation
            .outputs
            .iter()
            .enumerate()
            .find(|(index, port)| port.beside.is_none() && !written.contains(index))
            .map(|(_, port)| port)
        {
            return Err(CommandError::new(format!(
                "command for `{}` must use `{{{}}}`",
                operation.name,
                if port.name == DEFAULT_OUTPUT {
                    "@output"
                } else {
                    &port.name
                }
            )));
        }
    }
    Ok(())
}

/// Check that every step calling `operation` reads, at its input `index`, a
/// product with `dimension`, whose value each of its artifacts then has.
fn check_input_dimension(
    operation: &OperationDef,
    index: usize,
    name: &str,
    dimension: &str,
    callers: &Callers<'_, '_>,
) -> Result<(), CommandError> {
    let port = &operation.inputs[index].name;
    for caller in callers.steps {
        let Some(product) = caller
            .invocation
            .inputs
            .get(index)
            .and_then(|binding| callers.products.get(binding.product_name()))
        else {
            continue;
        };
        if product.dimensions.iter().any(|d| d == dimension) {
            continue;
        }
        let has = names_or_none(&product.dimensions);
        return Err(CommandError::new(format!(
            "command for `{}` uses `{{{name}}}`, but `{}`, which `{port}` reads in the step making `{}`, has no dimension `{dimension}`; it has {has}",
            operation.name, product.name, caller.product.name
        ))
        .focus(format!("{{{name}}}")));
    }
    Ok(())
}

/// `dimensions` as a list of names in backticks, or "no dimensions".
fn names_or_none(dimensions: &[String]) -> String {
    if dimensions.is_empty() {
        return "no dimensions".to_owned();
    }
    let names: Vec<_> = dimensions.iter().map(|name| format!("`{name}`")).collect();
    names.join(", ")
}

/// Check that every step calling `operation` gives its jobs a value of
/// `dimension`, which its first output's product has. An operation no step
/// calls, as in a file of operations for others to import, is checked when
/// one does.
fn check_dimension(
    operation: &OperationDef,
    dimension: &str,
    callers: &Callers<'_, '_>,
) -> Result<(), CommandError> {
    let Some(Caller {
        invocation,
        product,
    }) = callers
        .steps
        .iter()
        .find(|caller| !caller.product.dimensions.iter().any(|d| d == dimension))
    else {
        return Ok(());
    };
    let has = names_or_none(&product.dimensions);
    let step = format!("the jobs of `{}` have {has}", product.name);
    let varied = operation
        .inputs
        .iter()
        .zip(&invocation.inputs)
        .find(|(_, binding)| binding.vary.iter().any(|d| d == dimension));
    let message = match varied {
        Some((port, binding)) => format!(
            "command for `{}` uses `{{{dimension}}}`, but `{}` collects `{}` across `{dimension}` with `@vary({})`, so each job has many values of it; `{{{}.{dimension}}}` gives each artifact's, and {step}",
            operation.name,
            port.name,
            binding.product,
            binding.vary.join(", "),
            port.name,
        ),
        None if callers.dimensions.contains(dimension) => format!(
            "command for `{}` uses dimension `{{{dimension}}}`, which its jobs lack: {step}",
            operation.name
        ),
        None => format!(
            "command for `{}` uses unknown placeholder `{{{dimension}}}`: `{}` has no port `{dimension}`, and {step}",
            operation.name, operation.name
        ),
    };
    Err(CommandError::new(message).focus(format!("{{{dimension}}}")))
}

/// Check the ports a command's stdin and stdout are redirected to: stdin
/// reads one input's file, and stdout writes one output's, which counts as
/// using it. The output's index, for stdout.
fn redirected_ports(
    template: &CommandTemplate,
    operation: &OperationDef,
    written: &mut BTreeSet<usize>,
) -> Result<Option<usize>, CommandError> {
    let mut stdout = None;
    for redirect in template.redirects() {
        // The parser makes sure each is followed by one placeholder.
        let Some(name) = redirect.placeholder() else {
            continue;
        };
        let sign = redirect.stream.sign();
        let fail = |message: String| Err(CommandError::new(message).focus(format!("{{{name}}}")));
        let (port, facet) = facet(name);
        let Some(slot) = slot(operation, port).filter(|slot| !matches!(slot, Slot::Dimension))
        else {
            return fail(format!(
                "command for `{}` uses unknown placeholder `{{{name}}}`",
                operation.name
            ));
        };
        if let Slot::Cores = slot {
            return fail(format!(
                "`{sign} {{{name}}}`: `{{@cores}}` is the number of cores the job uses, not a file; a command's {} one of its operation's {}",
                if redirect.stream == Stream::Stdin { "stdin reads" } else { "stdout is written to" },
                if redirect.stream == Stream::Stdin { "inputs" } else { "outputs" },
            ));
        }
        if facet != Facet::Path {
            return fail(format!(
                "`{sign} {{{name}}}`: a command's stdin and stdout are files, so write `{sign} {{{port}}}`"
            ));
        }
        match (redirect.stream, slot) {
            (Stream::Stdout, Slot::Output(index)) => {
                if operation.outputs[index].folder {
                    return fail(format!(
                        "`> {{{name}}}`: `{port}` is a folder, and a command's stdout is one file"
                    ));
                }
                written.insert(index);
                stdout = Some(index);
            }
            (Stream::Stdout, Slot::Input(_)) => {
                return fail(format!(
                    "`> {{{name}}}`: `{port}` is an input of `{}`, and a command's stdout is written to one of its outputs",
                    operation.name
                ));
            }
            (Stream::Stdin, Slot::Input(index)) => {
                if operation.inputs[index].cardinality == Cardinality::Many {
                    return fail(format!(
                        "`< {{{name}}}`: `{port}` is a many input, and a command's stdin reads one file"
                    ));
                }
            }
            (_, Slot::Cores | Slot::Dimension) => {
                unreachable!("`{{@cores}}` and a dimension are rejected as a redirection above")
            }
            (Stream::Stdin, Slot::Output(_)) => {
                return fail(format!(
                    "`< {{{name}}}`: `{port}` is an output of `{}`, and a command's stdin reads one of its inputs",
                    operation.name
                ));
            }
        }
    }
    Ok(stdout)
}
