//! Plain-text signatures laid out at port boundaries for hover readers.

use crate::model::{
    Cardinality, CheckDef, CheckUse, OperationDef, OutputPort, ProductDef, DEFAULT_OUTPUT,
};
use crate::types::TypeExpr;
use std::fmt::Write;

pub(super) fn product_signature(product: &ProductDef, ty: &TypeExpr) -> String {
    format!(
        "{}: {ty}{} [{}]{}",
        product.name,
        ending(product.extension.as_deref(), product.folder),
        product.dimensions.join(", "),
        checks_text(&product.checks)
    )
}

/// ` @ check(a, b(1))` for the checks a port or source attaches, or nothing.
fn checks_text(checks: &[CheckUse]) -> String {
    if checks.is_empty() {
        return String::new();
    }
    let checks: Vec<_> = checks.iter().map(ToString::to_string).collect();
    format!(" @ check({})", checks.join(", "))
}

pub(super) fn check_signature(check: &CheckDef) -> String {
    let parameters = if check.parameters.is_empty() {
        String::new()
    } else {
        format!("({})", check.parameters.join(", "))
    };
    format!("check {}{parameters}: {}", check.name, check.template)
}

/// What follows a type in a declaration: ` .nii.gz`, ` /` for a folder,
/// ` .zarr/`, or nothing.
fn ending(extension: Option<&str>, folder: bool) -> String {
    let slash = if folder { "/" } else { "" };
    match extension {
        Some(extension) => format!(" {extension}{slash}"),
        None if folder => " /".to_owned(),
        None => String::new(),
    }
}

pub(super) fn operation_signature(operation: &OperationDef) -> String {
    let inputs = operation
        .inputs
        .iter()
        .map(|port| {
            let many = port.cardinality == Cardinality::Many;
            let mut text = format!(
                "{}: {}{}",
                port.name,
                if many { "many " } else { "" },
                port.artifact_type
            );
            // An operation has at most one many input, which its minimum counts.
            if let (true, Some(minimum)) = (many, operation.minimum_collection) {
                let _ = write!(text, " @ min({minimum})");
            }
            text.push_str(&checks_text(&port.checks));
            text
        })
        .collect::<Vec<_>>();
    let named = operation.outputs.len() != 1 || operation.outputs[0].name != DEFAULT_OUTPUT;
    let output_ports: Vec<_> = operation
        .outputs
        .iter()
        .map(|port| output_signature(port, named))
        .collect();
    let outputs = if named {
        format!("({})", output_ports.join(", "))
    } else {
        output_ports.join("")
    };
    let compact = format!(
        "operation {}({}) -> {outputs}",
        operation.name,
        inputs.join(", ")
    );
    if compact.chars().count() <= 88 && operation.outputs.len() <= 1 {
        return compact;
    }
    let inputs = inputs.join(",\n    ");
    let outputs = if named {
        format!("(\n    {}\n)", output_ports.join(",\n    "))
    } else {
        outputs
    };
    format!(
        "operation {}(\n    {inputs}\n) -> {outputs}",
        operation.name
    )
}

fn output_signature(port: &OutputPort, named: bool) -> String {
    let mut result = if named {
        format!("{}: {}", port.name, port.artifact_type)
    } else {
        port.artifact_type.to_string()
    };
    if let Some(beside) = &port.beside {
        if beside.suffix.starts_with('.') {
            let _ = write!(result, " {} beside {}", beside.suffix, beside.sibling);
        } else {
            let _ = write!(result, " \"{}\" beside {}", beside.suffix, beside.sibling);
        }
    } else {
        result.push_str(&ending(port.extension.as_deref(), port.folder));
    }
    result.push_str(&checks_text(&port.checks));
    result
}
