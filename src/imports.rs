//! Resolve file imports and merge their selected definitions into a pipeline.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{Pipeline, SourceInventory};
use crate::parser::{parse_document_with_imports, parse_use, strip_comment, ParseError, UseSpec};

pub(crate) fn apply_import(
    pipeline: &mut Pipeline,
    imports: &BTreeMap<usize, Pipeline>,
    line: usize,
) -> Result<(), ParseError> {
    let imported = imports.get(&line).ok_or_else(|| {
        ParseError::new(
            line,
            "imports require a document path; use parse_document_at",
        )
    })?;
    for product in &imported.products {
        if pipeline
            .products
            .iter()
            .any(|existing| existing.name == product.name)
        {
            return Err(ParseError::new(
                line,
                format!("import conflicts with product `{}`", product.name),
            ));
        }
    }
    for operation in &imported.operations {
        if pipeline
            .operations
            .iter()
            .any(|existing| existing.name == operation.name)
        {
            return Err(ParseError::new(
                line,
                format!("import conflicts with operation `{}`", operation.name),
            ));
        }
    }
    for command in &imported.commands {
        if pipeline
            .commands
            .iter()
            .any(|existing| existing.operation == command.operation)
        {
            return Err(ParseError::new(
                line,
                format!(
                    "import conflicts with command for operation `{}`",
                    command.operation
                ),
            ));
        }
    }
    for (product, template) in &imported.product_paths {
        if pipeline
            .product_paths
            .insert(product.clone(), template.clone())
            .is_some()
        {
            return Err(ParseError::new(
                line,
                format!("import conflicts with path for product `{product}`"),
            ));
        }
    }
    for product in &imported.products {
        pipeline
            .source_lines
            .products
            .insert(product.name.clone(), line);
    }
    for operation in &imported.operations {
        pipeline
            .source_lines
            .operations
            .insert(operation.name.clone(), line);
    }
    for constraint in &imported.constraints {
        pipeline
            .source_lines
            .constraints
            .insert(constraint.product.clone(), line);
        pipeline.source_lines.constraint_lines.push(line);
    }
    pipeline.products.extend(imported.products.iter().cloned());
    pipeline
        .operations
        .extend(imported.operations.iter().cloned());
    pipeline.commands.extend(imported.commands.iter().cloned());
    pipeline
        .constraints
        .extend(imported.constraints.iter().cloned());
    Ok(())
}

fn select_import(module: &Pipeline, spec: &UseSpec, line: usize) -> Result<Pipeline, ParseError> {
    let mut selected = Pipeline::default();
    for name in &spec.names {
        let mut operations = module
            .operations
            .iter()
            .filter(|operation| operation.name == *name);
        let operation = operations.next();
        let mut sources = module.products.iter().filter(|product| {
            product.name == *name
                && !module
                    .invocations
                    .iter()
                    .any(|invocation| invocation.output_product == *name)
        });
        let source = sources.next();
        if operations.next().is_some() || sources.next().is_some() {
            return Err(ParseError::new(
                line,
                format!("imported file has duplicate definition `{name}`"),
            ));
        }
        if operation.is_some() && source.is_some() {
            return Err(ParseError::new(
                line,
                format!("import name `{name}` matches both a source and an operation"),
            ));
        }
        let qualified = spec
            .alias
            .as_ref()
            .map_or_else(|| name.clone(), |alias| format!("{alias}::{name}"));
        if let Some(operation) = operation {
            if module
                .commands
                .iter()
                .filter(|command| command.operation == *name)
                .count()
                > 1
            {
                return Err(ParseError::new(
                    line,
                    format!("imported file has duplicate command for `{name}`"),
                ));
            }
            if selected
                .operations
                .iter()
                .any(|existing| existing.name == qualified)
            {
                return Err(ParseError::new(line, format!("duplicate import `{name}`")));
            }
            let mut operation = operation.clone();
            operation.name = qualified.clone();
            selected.operations.push(operation);
            for command in module
                .commands
                .iter()
                .filter(|command| command.operation == *name)
            {
                let mut command = command.clone();
                command.operation = qualified.clone();
                selected.commands.push(command);
            }
        } else if let Some(source) = source {
            if selected
                .products
                .iter()
                .any(|existing| existing.name == qualified)
            {
                return Err(ParseError::new(line, format!("duplicate import `{name}`")));
            }
            let mut source = source.clone();
            source.name = qualified.clone();
            selected.products.push(source);
            if let Some(path) = module
                .product_paths
                .get(name)
                .or(module.path_template.as_ref())
            {
                selected
                    .product_paths
                    .insert(qualified.clone(), path.replace("{product}", name));
            }
            for constraint in module
                .constraints
                .iter()
                .filter(|constraint| constraint.product == *name)
            {
                let mut constraint = constraint.clone();
                constraint.product = qualified.clone();
                selected.constraints.push(constraint);
            }
        } else {
            return Err(ParseError::new(
                line,
                format!("`{name}` is not a source or operation in `{}`", spec.path),
            ));
        }
    }
    Ok(selected)
}

/// Parse a pipeline from a known file location, resolving named imports.
/// Import paths are relative to the file that contains each `use` line.
pub fn parse_document_at(
    text: &str,
    path: &Path,
) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    let root = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    parse_document_at_inner(text, &root, &mut vec![root.clone()])
}

fn parse_document_at_inner(
    text: &str,
    path: &Path,
    stack: &mut Vec<PathBuf>,
) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    let mut imports = BTreeMap::new();
    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        if !line.starts_with("use ") {
            continue;
        }
        let number = index + 1;
        let spec = parse_use(line, number)?;
        let imported_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&spec.path);
        let canonical = fs::canonicalize(&imported_path).map_err(|error| {
            ParseError::new(
                number,
                format!("cannot load import `{}`: {error}", imported_path.display()),
            )
        })?;
        if stack.contains(&canonical) {
            return Err(ParseError::new(
                number,
                format!("import cycle through `{}`", canonical.display()),
            ));
        }
        let imported_text = fs::read_to_string(&canonical).map_err(|error| {
            ParseError::new(
                number,
                format!("cannot read import `{}`: {error}", canonical.display()),
            )
        })?;
        stack.push(canonical.clone());
        let module = parse_document_at_inner(&imported_text, &canonical, stack).map_err(|error| {
            ParseError::new(
                number,
                format!(
                    "in `{}` at line {}: {}",
                    canonical.display(),
                    error.line,
                    error.message
                ),
            )
        });
        stack.pop();
        let (module, _) = module?;
        imports.insert(number, select_import(&module, &spec, number)?);
    }
    parse_document_with_imports(text, &imports)
}
