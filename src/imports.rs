//! Resolve file imports and merge their selected definitions into a pipeline.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::{FxHashMap, FxHashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::blob::git_blob_id;
use crate::lower::{parse_document_with_imports, ParsedDocument, PipelineBuilder};
use crate::model::{
    CheckDef, CheckUse, CommandDef, CommandRole, OperationDef, Pipeline, PipelineIndex, ProductDef,
    SidecarGroup, SourceFile,
};
use crate::parser::{parse_use, strip_comment, without_bom, Keyword, Kind, ParseError, UseSpec};
use crate::span::Place;

pub(crate) fn apply_import(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    place: &Place,
) -> Result<(), ParseError> {
    let line = place.line;
    let pipeline = &mut builder.pipeline;
    let imported = imports.get(&line).ok_or_else(|| {
        ParseError::new(
            line,
            "imports require a document path; use parse_pipeline_at",
        )
    })?;
    for ((kind, existing), (_, imported)) in defined(pipeline).into_iter().zip(defined(imported)) {
        let existing: FxHashSet<_> = existing.into_iter().collect();
        if let Some(name) = imported.into_iter().find(|name| existing.contains(name)) {
            return Err(ParseError::new(
                line,
                format!("import conflicts with {kind} `{name}`"),
            ));
        }
    }
    pipeline.product_paths.extend(
        imported
            .product_paths
            .iter()
            .map(|(product, template)| (product.clone(), template.clone())),
    );
    for command in &imported.commands {
        builder.add_command(command.clone(), place.clone());
    }
    // Two imports may bring the same check, which is one check; a
    // different check of the same name is a conflict.
    for check in &imported.checks {
        match builder
            .pipeline
            .checks
            .iter()
            .find(|c| c.name == check.name)
        {
            Some(existing) if existing == check => {}
            Some(_) => {
                return Err(ParseError::new(
                    line,
                    format!("import conflicts with check `{}`", check.name),
                ))
            }
            None => builder.add_check(check.clone(), place.clone()),
        }
    }
    for group in &imported.sidecar_groups {
        builder.add_sidecar_group(group.clone(), place.clone());
    }
    for product in &imported.products {
        builder.add_product(product.clone(), place.clone());
    }
    for operation in &imported.operations {
        builder.add_operation(operation.clone(), place.clone(), None)?;
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
    Ok(())
}

/// What `pipeline` defines that an import may not define again, by kind.
fn defined(pipeline: &Pipeline) -> [(&'static str, Vec<&str>); 5] {
    let products = pipeline
        .products
        .iter()
        .map(|product| product.name.as_str());
    let operations = pipeline.operations.iter();
    let commands = pipeline.commands.iter();
    [
        ("product", products.collect()),
        (
            "operation",
            operations
                .map(|operation| operation.name.as_str())
                .collect(),
        ),
        (
            "command for operation",
            commands.map(|command| command.operation.as_str()).collect(),
        ),
        (
            "path for product",
            pipeline.product_paths.keys().map(String::as_str).collect(),
        ),
        (
            "sidecars group",
            pipeline
                .sidecar_groups
                .iter()
                .map(|group| group.name.as_str())
                .collect(),
        ),
    ]
}

/// An imported file's operations, sources, checks and commands by name, found
/// once, so that each name an import selects is looked up, not searched for.
struct Module<'m> {
    index: PipelineIndex<'m>,
    /// Every operation of a name, to find a repeat.
    operations: FxHashMap<&'m str, Vec<&'m OperationDef>>,
    /// Every source of a name.
    sources: FxHashMap<&'m str, Vec<&'m ProductDef>>,
    /// Each `sidecars` group by name.
    groups: FxHashMap<&'m str, &'m SidecarGroup>,
    /// Each member of a `sidecars` group, with its group.
    members: FxHashMap<&'m str, &'m SidecarGroup>,
    /// The first check of a name.
    checks: FxHashMap<&'m str, &'m CheckDef>,
    /// Each operation's commands, in order.
    commands: FxHashMap<&'m str, Vec<&'m CommandDef>>,
}

impl<'m> Module<'m> {
    fn new(module: &'m Pipeline) -> Self {
        let index = PipelineIndex::new(module);
        let mut operations: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for operation in &module.operations {
            operations
                .entry(operation.name.as_str())
                .or_default()
                .push(operation);
        }
        let mut sources: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for product in &module.products {
            if index.is_source(&product.name) {
                sources
                    .entry(product.name.as_str())
                    .or_default()
                    .push(product);
            }
        }
        let mut checks = FxHashMap::default();
        for check in &module.checks {
            checks.entry(check.name.as_str()).or_insert(check);
        }
        let mut commands: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for command in &module.commands {
            commands
                .entry(command.operation.as_str())
                .or_default()
                .push(command);
        }
        let groups = module
            .sidecar_groups
            .iter()
            .map(|group| (group.name.as_str(), group))
            .collect();
        Self {
            index,
            operations,
            sources,
            groups,
            members: module.sidecar_members(),
            checks,
            commands,
        }
    }
}

/// What an import has selected so far, with the names it holds, so that a
/// repeat is found by lookup.
#[derive(Default)]
struct Selection {
    pipeline: Pipeline,
    operations: FxHashSet<String>,
    products: FxHashSet<String>,
    checks: FxHashSet<String>,
}

fn select_import(module: &Pipeline, spec: &UseSpec, line: usize) -> Result<Pipeline, ParseError> {
    let module = Module::new(module);
    let mut selected = Selection::default();
    let import_all = spec.names.is_none();
    let names: Vec<&str> = match &spec.names {
        Some(names) => names.iter().map(String::as_str).collect(),
        None => reusable_names(&module.index),
    };
    if names.is_empty() {
        return Err(ParseError::new(
            line,
            format!("`{}` contains no reusable definitions", spec.path),
        ));
    }
    for name in names {
        // A member comes with its group, and is no source of its own.
        if let Some(group) = module.members.get(name) {
            return Err(ParseError::new(
                line,
                format!(
                    "`{name}` is a member of sidecars group `{}`; import the group, `{}`, to bring its members",
                    group.name, group.name
                ),
            ));
        }
        let operations = module.operations.get(name).map_or(&[][..], Vec::as_slice);
        let sources = module.sources.get(name).map_or(&[][..], Vec::as_slice);
        let group = module.groups.get(name).copied();
        if operations.len() > 1 || sources.len() > 1 {
            return Err(ParseError::new(
                line,
                format!("imported file has duplicate definition `{name}`"),
            ));
        }
        let (operation, source) = (operations.first().copied(), sources.first().copied());
        let check = module.checks.get(name).copied();
        if operation.is_none() && source.is_none() && group.is_none() && check.is_none() {
            return Err(ParseError::new(
                line,
                format!(
                    "`{name}` is not a source, operation, sidecars group or check in `{}`",
                    spec.path
                ),
            ));
        }
        if operation.is_some() && (source.is_some() || group.is_some()) && !import_all {
            return Err(ParseError::new(
                line,
                format!("import name `{name}` matches both a source and an operation"),
            ));
        }
        let qualified = spec
            .alias
            .as_ref()
            .map_or_else(|| name.to_owned(), |alias| format!("{alias}::{name}"));
        let alias = spec.alias.as_deref();
        if let Some(operation) = operation {
            import_operation(&mut selected, &module, operation, spec, line)?;
        }
        if let Some(source) = source {
            import_source(&mut selected, &module, source, &qualified, line)?;
            for used in &source.checks {
                import_check(&mut selected, &module, &used.check, alias);
            }
        }
        if let Some(group) = group {
            import_group(&mut selected, &module, group, spec.alias.as_deref(), line)?;
        }
        if check.is_some() {
            import_check(&mut selected, &module, name, alias);
        }
    }
    import_called(&mut selected, &module, spec, line)?;
    qualify_checks(&mut selected.pipeline, spec.alias.as_deref());
    Ok(selected.pipeline)
}

/// Bring in every operation the bodies of the selected operations call,
/// and those their bodies call in turn, under the same alias: a body's
/// steps call what its own file declares.
fn import_called(
    selected: &mut Selection,
    module: &Module<'_>,
    spec: &UseSpec,
    line: usize,
) -> Result<(), ParseError> {
    // The selected operations' bodies, as their own file writes them.
    let alias = spec.alias.as_deref();
    let local = |qualified: &str| match alias {
        Some(alias) => qualified
            .strip_prefix(alias)
            .and_then(|rest| rest.strip_prefix("::"))
            .unwrap_or(qualified)
            .to_owned(),
        None => qualified.to_owned(),
    };
    let mut pending: Vec<&str> = selected
        .pipeline
        .operations
        .iter()
        .filter_map(|operation| module.operations.get(local(&operation.name).as_str()))
        .filter_map(|operations| operations.first())
        .flat_map(|operation| &operation.steps)
        .map(|step| step.invocation.operation.as_str())
        .collect();
    // Every operation the bodies call, in turn, by its own file's name.
    let mut called = FxHashSet::default();
    while let Some(name) = pending.pop() {
        if !called.insert(name) {
            continue;
        }
        let operation = module
            .operations
            .get(name)
            .and_then(|operations| operations.first().copied())
            .expect("a body calls only operations its file declares before it");
        pending.extend(
            operation
                .steps
                .iter()
                .map(|step| step.invocation.operation.as_str()),
        );
    }
    // Brought in the order their file declares them.
    for operation in &module.index.pipeline.operations {
        let name = qualified_check(&operation.name, alias);
        if called.contains(operation.name.as_str()) && !selected.operations.contains(&name) {
            import_operation(selected, module, operation, spec, line)?;
        }
    }
    Ok(())
}

/// Bring in `module`'s check `name`, once however many imports use it.
fn import_check(selected: &mut Selection, module: &Module<'_>, name: &str, alias: Option<&str>) {
    let qualified = qualified_check(name, alias);
    if selected.checks.contains(&qualified) {
        return;
    }
    if let Some(check) = module.checks.get(name) {
        selected.checks.insert(qualified.clone());
        selected.pipeline.checks.push(CheckDef {
            name: qualified,
            ..(*check).clone()
        });
    }
}

fn qualified_check(name: &str, alias: Option<&str>) -> String {
    alias.map_or_else(|| name.to_owned(), |alias| format!("{alias}::{name}"))
}

/// Name the checks the imported ports and sources use as they are imported.
fn qualify_checks(selected: &mut Pipeline, alias: Option<&str>) {
    if alias.is_none() {
        return;
    }
    let qualify = |uses: &mut Vec<CheckUse>| {
        for used in uses {
            used.check = qualified_check(&used.check, alias);
        }
    };
    for operation in &mut selected.operations {
        operation
            .inputs
            .iter_mut()
            .for_each(|port| qualify(&mut port.checks));
        operation
            .outputs
            .iter_mut()
            .for_each(|port| qualify(&mut port.checks));
    }
    selected
        .products
        .iter_mut()
        .for_each(|product| qualify(&mut product.checks));
}

/// What `use path` brings in: every operation, source and check, each name
/// once.
fn reusable_names<'m>(index: &PipelineIndex<'m>) -> Vec<&'m str> {
    let module = index.pipeline;
    let members = module.sidecar_members();
    let mut seen = BTreeSet::new();
    module
        .operations
        .iter()
        .map(|operation| operation.name.as_str())
        .chain(
            module
                .products
                .iter()
                .filter(|product| index.is_source(&product.name))
                .filter(|product| !members.contains_key(product.name.as_str()))
                .map(|product| product.name.as_str()),
        )
        .chain(
            module
                .sidecar_groups
                .iter()
                .map(|group| group.name.as_str()),
        )
        .chain(module.checks.iter().map(|check| check.name.as_str()))
        .filter(|name| seen.insert(*name))
        .collect()
}

/// Import an operation under `spec`'s alias, with its commands, the checks
/// its ports attach, and the file it is declared in. The steps of its body,
/// if it has one, call operations under the same alias.
fn import_operation(
    selected: &mut Selection,
    module: &Module<'_>,
    operation: &OperationDef,
    spec: &UseSpec,
    line: usize,
) -> Result<(), ParseError> {
    let name = &operation.name;
    let alias = spec.alias.as_deref();
    let qualified = &qualified_check(name, alias);
    let commands = module
        .commands
        .get(name.as_str())
        .map_or(&[][..], Vec::as_slice);
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
    if !selected.operations.insert(qualified.to_owned()) {
        return Err(ParseError::new(line, format!("duplicate import `{name}`")));
    }
    let mut steps = operation.steps.clone();
    for step in &mut steps {
        step.invocation.operation = qualified_check(&step.invocation.operation, alias);
    }
    selected.pipeline.operations.push(OperationDef {
        name: qualified.to_owned(),
        steps,
        file: Some(imported_file(operation.file.as_deref(), &spec.path)),
        ..operation.clone()
    });
    selected
        .pipeline
        .commands
        .extend(commands.iter().map(|command| CommandDef {
            operation: qualified.to_owned(),
            ..(*command).clone()
        }));
    let checks = operation.inputs.iter().map(|port| &port.checks);
    let checks = checks.chain(operation.outputs.iter().map(|port| &port.checks));
    for used in checks.flatten() {
        import_check(selected, module, &used.check, alias);
    }
    Ok(())
}

/// The file an operation is declared in, relative to the file that imports
/// it through `use path`: `path` itself, or, for one `path` imports in
/// turn, that file relative to `path`'s folder.
fn imported_file(declared_in: Option<&str>, path: &str) -> String {
    let path = path.replace('\\', "/");
    let Some(declared_in) = declared_in else {
        return path;
    };
    match path.rsplit_once('/') {
        Some((folder, _)) => format!("{folder}/{declared_in}"),
        None => declared_in.to_owned(),
    }
}

/// Import a source as `qualified`, with its path rule.
fn import_source(
    selected: &mut Selection,
    module: &Module<'_>,
    source: &ProductDef,
    qualified: &str,
    line: usize,
) -> Result<(), ParseError> {
    let name = &source.name;
    if !selected.products.insert(qualified.to_owned()) {
        return Err(ParseError::new(line, format!("duplicate import `{name}`")));
    }
    selected.pipeline.products.push(ProductDef {
        name: qualified.to_owned(),
        ..source.clone()
    });
    // With the extension its own file's `ext:` gives it, if any.
    if let Some(path) = module.index.path_template_for(name) {
        selected
            .pipeline
            .product_paths
            .insert(qualified.to_owned(), path.with_product(name));
    }
    Ok(())
}

/// Import a `sidecars` group as `alias::name`, whole: its members become the
/// sources `alias::member`, each with its path and checks, as when imported
/// on its own, so the group's diagnostics and a recipe's stem for it work as
/// they do in the file the group is written in.
fn import_group(
    selected: &mut Selection,
    module: &Module<'_>,
    group: &SidecarGroup,
    alias: Option<&str>,
    line: usize,
) -> Result<(), ParseError> {
    let qualify = |name: &str| qualified_check(name, alias);
    let mut members = Vec::new();
    for (member, extension) in &group.members {
        let source = module
            .sources
            .get(member.as_str())
            .and_then(|sources| sources.first().copied())
            .expect("a sidecars group's members are declared as sources");
        import_source(selected, module, source, &qualify(member), line)?;
        for used in &source.checks {
            import_check(selected, module, &used.check, alias);
        }
        members.push((qualify(member), extension.clone()));
    }
    selected.pipeline.sidecar_groups.push(SidecarGroup {
        name: qualify(&group.name),
        dimensions: group.dimensions.clone(),
        members,
        stem: group.stem.clone(),
    });
    Ok(())
}

/// Parse a pipeline from a known file location, resolving imports.
/// Import paths are relative to the file that contains each `use` line.
pub fn parse_pipeline_at(text: &str, path: &Path) -> Result<Pipeline, ParseError> {
    parse_located_document(text, path, Kind::Pipeline).map(|document| document.pipeline)
}

/// Parse a pipeline or a recipe at `path`, keeping declaration lines.
pub(crate) fn parse_located_document(
    text: &str,
    path: &Path,
    kind: Kind,
) -> Result<ParsedDocument, ParseError> {
    let root = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    parse_document_at_inner(text, &root, &mut vec![root.clone()], kind)
}

/// Parse `text`, the file at `path` as read, with its imports. The parsed
/// pipeline lists the file first among its files, then each file an
/// import read, relative to `path`'s folder.
fn parse_document_at_inner(
    raw: &str,
    path: &Path,
    stack: &mut Vec<PathBuf>,
    kind: Kind,
) -> Result<ParsedDocument, ParseError> {
    let text = without_bom(raw);
    let mut files = vec![SourceFile {
        path: path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        // The file as git holds it: the text given may be rebuilt from its
        // lines, as error recovery does, without its last newline.
        blob: fs::read(path)
            .map_or_else(|_| git_blob_id(raw.as_bytes()), |bytes| git_blob_id(&bytes)),
    }];
    let mut imports = BTreeMap::new();
    let mut file_texts = BTreeMap::new();
    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        if Keyword::of(line) != Some(Keyword::Use) {
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
        stack.push(canonical.clone());
        let module = parse_document_at_inner(&imported_text, &canonical, stack, Kind::Pipeline)
            .map_err(|error| {
                ParseError::new(
                    number,
                    format!(
                        "in `{}` at line {}: {}",
                        canonical.display(),
                        error.line(),
                        error.message()
                    ),
                )
            });
        stack.pop();
        let module = module?;
        let folder = spec.path.replace('\\', "/");
        let folder = folder.rsplit_once('/').map(|(folder, _)| folder);
        let rebased = |path: &str| match folder {
            Some(folder) => format!("{folder}/{path}"),
            None => path.to_owned(),
        };
        if let Some(own) = module.pipeline.files.first() {
            file_texts.insert(rebased(&own.path), without_bom(&imported_text).to_owned());
        }
        for (path, text) in &module.lines.file_texts {
            file_texts.insert(rebased(path), text.clone());
        }
        for file in &module.pipeline.files {
            let path = rebased(&file.path);
            if !files.iter().any(|known: &SourceFile| known.path == path) {
                files.push(SourceFile {
                    path,
                    blob: file.blob.clone(),
                });
            }
        }
        imports.insert(number, select_import(&module.pipeline, &spec, number)?);
    }
    let mut document = parse_document_with_imports(text, &imports, kind)?;
    document.pipeline.files = files;
    document.lines.file_texts = file_texts;
    Ok(document)
}
