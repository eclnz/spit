//! Resolve file imports and merge their selected definitions into a pipeline.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::lower::{parse_document_with_imports, ParsedDocument, PipelineBuilder};
use crate::model::{
    CommandDef, CommandRole, CoverageRule, OperationDef, Pipeline, ProductDef, SourceInventory,
};
use crate::parser::{parse_use, strip_comment, InlineInventory, ParseError, Rule, UseSpec};
use crate::span::Place;

pub(crate) fn apply_import(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    place: Place,
) -> Result<(), ParseError> {
    let line = place.line;
    let pipeline = &mut builder.pipeline;
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
    for command in &imported.commands {
        builder.add_command(command.clone(), place.clone());
    }
    for product in &imported.products {
        builder.add_product(product.clone(), place.clone());
    }
    for operation in &imported.operations {
        builder.add_operation(operation.clone(), place.clone());
    }
    let lines = &mut builder.lines;
    lines.imported.extend(
        imported
            .products
            .iter()
            .map(|product| product.name.clone())
            .chain(
                imported
                    .operations
                    .iter()
                    .map(|operation| operation.name.clone()),
            ),
    );
    for product in imported.product_paths.keys() {
        lines.paths.insert(product.clone(), place.clone());
    }
    for constraint in &imported.constraints {
        builder.add_constraint(constraint.clone(), Rule::spanning(&place));
    }
    Ok(())
}

fn select_import(module: &Pipeline, spec: &UseSpec, line: usize) -> Result<Pipeline, ParseError> {
    let mut selected = Pipeline::default();
    let import_all = spec.names.is_none();
    let names: Vec<&str> = match &spec.names {
        Some(names) => names.iter().map(String::as_str).collect(),
        None => reusable_names(module),
    };
    if names.is_empty() {
        return Err(ParseError::new(
            line,
            format!("`{}` contains no reusable definitions", spec.path),
        ));
    }
    for name in names {
        let mut operations = module
            .operations
            .iter()
            .filter(|operation| operation.name == name);
        let operation = operations.next();
        let mut sources = module
            .products
            .iter()
            .filter(|product| product.name == name && is_source(module, product));
        let source = sources.next();
        if operations.next().is_some() || sources.next().is_some() {
            return Err(ParseError::new(
                line,
                format!("imported file has duplicate definition `{name}`"),
            ));
        }
        if operation.is_none() && source.is_none() {
            return Err(ParseError::new(
                line,
                format!("`{name}` is not a source or operation in `{}`", spec.path),
            ));
        }
        if operation.is_some() && source.is_some() && !import_all {
            return Err(ParseError::new(
                line,
                format!("import name `{name}` matches both a source and an operation"),
            ));
        }
        let qualified = spec
            .alias
            .as_ref()
            .map_or_else(|| name.to_owned(), |alias| format!("{alias}::{name}"));
        if let Some(operation) = operation {
            import_operation(&mut selected, module, operation, &qualified, line)?;
        }
        if let Some(source) = source {
            import_source(&mut selected, module, source, &qualified, line)?;
        }
    }
    Ok(selected)
}

/// What `use path` brings in: every operation and source, each name once.
fn reusable_names(module: &Pipeline) -> Vec<&str> {
    let mut seen = BTreeSet::new();
    module
        .operations
        .iter()
        .map(|operation| operation.name.as_str())
        .chain(
            module
                .products
                .iter()
                .filter(|product| is_source(module, product))
                .map(|product| product.name.as_str()),
        )
        .filter(|name| seen.insert(*name))
        .collect()
}

/// Whether `product` is a source, which no step produces.
fn is_source(module: &Pipeline, product: &ProductDef) -> bool {
    !module
        .invocations
        .iter()
        .any(|invocation| invocation.outputs.contains(&product.name))
}

/// Import an operation as `qualified`, with its commands.
fn import_operation(
    selected: &mut Pipeline,
    module: &Pipeline,
    operation: &OperationDef,
    qualified: &str,
    line: usize,
) -> Result<(), ParseError> {
    let name = &operation.name;
    let commands: Vec<_> = module
        .commands
        .iter()
        .filter(|command| &command.operation == name)
        .collect();
    if commands
        .iter()
        .filter(|command| command.role == CommandRole::Run)
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
    selected.operations.push(OperationDef {
        name: qualified.to_owned(),
        ..operation.clone()
    });
    selected
        .commands
        .extend(commands.into_iter().map(|command| CommandDef {
            operation: qualified.to_owned(),
            ..command.clone()
        }));
    Ok(())
}

/// Import a source as `qualified`, with its path rule and coverage rules.
fn import_source(
    selected: &mut Pipeline,
    module: &Pipeline,
    source: &ProductDef,
    qualified: &str,
    line: usize,
) -> Result<(), ParseError> {
    let name = &source.name;
    if selected
        .products
        .iter()
        .any(|existing| existing.name == qualified)
    {
        return Err(ParseError::new(line, format!("duplicate import `{name}`")));
    }
    selected.products.push(ProductDef {
        name: qualified.to_owned(),
        ..source.clone()
    });
    if let Some(path) = module
        .product_paths
        .get(name)
        .or(module.path_template.as_ref())
    {
        selected
            .product_paths
            .insert(qualified.to_owned(), path.with_product(name));
    }
    selected.constraints.extend(
        module
            .constraints
            .iter()
            .filter(|constraint| &constraint.product == name)
            .map(|constraint| CoverageRule {
                product: qualified.to_owned(),
                ..constraint.clone()
            }),
    );
    Ok(())
}

/// Parse a pipeline from a known file location, resolving imports.
/// Import paths are relative to the file that contains each `use` line.
pub fn parse_document_at(
    text: &str,
    path: &Path,
) -> Result<(Pipeline, Option<SourceInventory>), ParseError> {
    parse_located_document(text, path, InlineInventory::Read)
        .map(|document| (document.pipeline, document.inventory))
}

/// Like [`parse_document_at`], but skips any inline inventory, for use with a
/// separate one that replaces it. The inline records need not parse.
pub fn parse_pipeline_at(text: &str, path: &Path) -> Result<Pipeline, ParseError> {
    parse_located_document(text, path, InlineInventory::Skip).map(|document| document.pipeline)
}

/// Like [`parse_document_at`], but also keeps declaration line numbers.
pub(crate) fn parse_located_document(
    text: &str,
    path: &Path,
    inline: InlineInventory,
) -> Result<ParsedDocument, ParseError> {
    let root = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    parse_document_at_inner(text, &root, &mut vec![root.clone()], inline)
}

fn parse_document_at_inner(
    text: &str,
    path: &Path,
    stack: &mut Vec<PathBuf>,
    inline: InlineInventory,
) -> Result<ParsedDocument, ParseError> {
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
        // A device such as `/dev/zero` would never finish reading.
        if !canonical.is_file() {
            return Err(ParseError::new(
                number,
                format!("import `{}` is not a regular file", canonical.display()),
            ));
        }
        let imported_text = fs::read_to_string(&canonical).map_err(|error| {
            ParseError::new(
                number,
                format!("cannot read import `{}`: {error}", canonical.display()),
            )
        })?;
        let imported_text = imported_text
            .strip_prefix('\u{feff}')
            .unwrap_or(&imported_text);
        stack.push(canonical.clone());
        // Imports bring no inventory records, so an imported file's are skipped.
        let module =
            parse_document_at_inner(imported_text, &canonical, stack, InlineInventory::Skip)
                .map_err(|error| {
                    ParseError::new(
                        number,
                        format!(
                            "in `{}` at line {}: {}",
                            canonical.display(),
                            error.line(),
                            error.message
                        ),
                    )
                });
        stack.pop();
        imports.insert(number, select_import(&module?.pipeline, &spec, number)?);
    }
    parse_document_with_imports(text, &imports, inline)
}
