use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::{FxHashMap, FxHashSet};
use std::fs;
use std::path::{Path, PathBuf};

mod kind;
mod located;
mod shared;

use super::{parse_layout, ParsedDocument, PipelineBuilder};
use crate::blob::git_blob_id;
use crate::model::{
    CheckDef, CommandDef, CommandRole, OperationDef, Pipeline, PipelineIndex, ProductDef,
    ResourcesDef, SidecarGroup, SourceFile,
};
use crate::parser::error::LowerError;
use crate::parser::{
    parse_use, without_bom, CommandPlaces, Keyword, Kind, Layout, Line, ParseError, UseSpec,
};
use crate::span::Place;
use kind::{first_conflict, DeclKind};
pub(crate) use located::relative_path;
use located::{in_import, named_in, rebase, ImportedAt};
use shared::{import_check, import_review, qualified_name, qualify_attached};

pub(crate) fn apply_import(
    builder: &mut PipelineBuilder,
    imports: &BTreeMap<usize, Pipeline>,
    place: &Place,
) -> Result<(), ParseError> {
    let line = place.line;
    let pipeline = &mut builder.pipeline;
    let imported = imports
        .get(&line)
        .ok_or_else(|| ParseError::of(line, LowerError::ImportWithoutPath))?;
    if let Some(error) = first_conflict(pipeline, imported) {
        return Err(ParseError::of(line, error));
    }
    pipeline.type_decls.extend(
        imported
            .type_decls
            .iter()
            .map(|(name, params)| (name.clone(), params.clone())),
    );
    pipeline.product_paths.extend(
        imported
            .product_paths
            .iter()
            .map(|(product, template)| (product.clone(), template.clone())),
    );
    for command in &imported.commands {
        let places = CommandPlaces {
            header: place.clone(),
            lines: vec![place.clone(); command.lines.len()],
        };
        builder.add_command(command.clone(), places);
    }
    for resources in &imported.resources {
        builder.add_resources(resources.clone(), place.clone());
    }
    for check in &imported.checks {
        if !(builder.pipeline.checks.iter()).any(|earlier| earlier.name == check.name) {
            builder.add_check(check.clone(), place.clone());
        }
    }
    for review in &imported.reviews {
        if !builder.pipeline.reviews.contains(review) {
            builder.add_review(review.clone(), place.clone());
        }
    }
    for group in &imported.sidecar_groups {
        builder.add_sidecar_group(group.clone());
    }
    for product in &imported.products {
        builder.add_product(product.clone(), place.clone());
    }
    for operation in &imported.operations {
        builder
            .add_operation(operation.clone(), place.clone(), None)
            .expect("an imported operation conflicting with one already declared is found above");
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

struct Module<'m> {
    index: PipelineIndex<'m>,
    operations: FxHashMap<&'m str, Vec<&'m OperationDef>>,
    sources: FxHashMap<&'m str, Vec<&'m ProductDef>>,
    groups: FxHashMap<&'m str, &'m SidecarGroup>,
    members: FxHashMap<&'m str, &'m SidecarGroup>,
    checks: FxHashMap<&'m str, &'m CheckDef>,
    reviews: FxHashSet<&'m str>,
    commands: FxHashMap<&'m str, Vec<&'m CommandDef>>,
    resources: FxHashMap<&'m str, Vec<&'m ResourcesDef>>,
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
        let mut resources: FxHashMap<_, Vec<_>> = FxHashMap::default();
        for line in &module.resources {
            resources
                .entry(line.operation.as_str())
                .or_default()
                .push(line);
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
            reviews: module
                .reviews
                .iter()
                .map(|review| review.name.as_str())
                .collect(),
            commands,
            resources,
        }
    }
}

#[derive(Default)]
struct Selection {
    pipeline: Pipeline,
    operations: FxHashSet<String>,
    products: FxHashSet<String>,
    checks: FxHashSet<String>,
    reviews: FxHashSet<String>,
}

fn select_import(module: &Pipeline, spec: &UseSpec, line: usize) -> Result<Pipeline, ParseError> {
    let module = Module::new(module);
    let mut selected = Selection::default();
    selected.pipeline.type_decls = module.index.pipeline.type_decls.clone();
    let import_all = spec.names.is_none();
    let names: Vec<&str> = match &spec.names {
        Some(names) => names.iter().map(String::as_str).collect(),
        None => reusable_names(&module.index),
    };
    if names.is_empty() {
        return Err(ParseError::of(
            line,
            LowerError::ImportHasNoDefinitions {
                path: spec.path.to_string(),
            },
        ));
    }
    for name in names {
        if let Some(group) = module.members.get(name).filter(|group| group.name != name) {
            return Err(ParseError::of(
                line,
                LowerError::ImportBesideSourceAlone {
                    name: name.to_string(),
                    group: group.name.to_string(),
                },
            ));
        }
        let operations = module.operations.get(name).map_or(&[][..], Vec::as_slice);
        let sources = module.sources.get(name).map_or(&[][..], Vec::as_slice);
        let group = module.groups.get(name).copied();
        if operations.len() > 1 || sources.len() > 1 {
            return Err(ParseError::of(
                line,
                LowerError::ImportedFileHasDuplicateDefinition {
                    name: name.to_string(),
                },
            ));
        }
        let (operation, source) = (operations.first().copied(), sources.first().copied());
        let check = module.checks.get(name).copied();
        let review = module.reviews.contains(name);
        if !(DeclKind::ALL.iter()).any(|kind| kind.selectable(&module, name)) {
            return Err(ParseError::of(
                line,
                LowerError::ImportNameNotFound {
                    name: name.to_string(),
                    path: spec.path.to_string(),
                },
            ));
        }
        if operation.is_some() && (source.is_some() || group.is_some()) && !import_all {
            return Err(ParseError::of(
                line,
                LowerError::ImportNameMatchesBothSourceOperation {
                    name: name.to_string(),
                },
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
        if let Some(source) = source.filter(|_| group.is_none()) {
            import_source(&mut selected, &module, source, &qualified, alias, line)?;
            for used in &source.checks {
                import_check(&mut selected, &module, &used.check, alias);
            }
            for review in &source.reviews {
                import_review(&mut selected, &module, review, alias);
            }
        }
        if let Some(group) = group {
            import_group(&mut selected, &module, group, spec.alias.as_deref(), line)?;
        }
        if check.is_some() {
            import_check(&mut selected, &module, name, alias);
        }
        if review {
            import_review(&mut selected, &module, name, alias);
        }
    }
    import_called(&mut selected, &module, spec, line)?;
    for product in &mut selected.pipeline.products {
        if let Some(folder) = &mut product.here {
            *folder = if folder.is_empty() {
                rebase(&spec.path, "")
                    .rsplit_once('/')
                    .map_or_else(String::new, |(parent, _)| parent.to_owned())
            } else {
                rebase(&spec.path, folder)
            };
        }
    }
    qualify_attached(&mut selected.pipeline, spec.alias.as_deref());
    Ok(selected.pipeline)
}

fn import_called(
    selected: &mut Selection,
    module: &Module<'_>,
    spec: &UseSpec,
    line: usize,
) -> Result<(), ParseError> {
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
    for operation in &module.index.pipeline.operations {
        let name = qualified_name(&operation.name, alias);
        if called.contains(operation.name.as_str()) && !selected.operations.contains(&name) {
            import_operation(selected, module, operation, spec, line)?;
        }
    }
    Ok(())
}

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
        .chain(module.reviews.iter().map(|review| review.name.as_str()))
        .filter(|name| seen.insert(*name))
        .collect()
}

fn import_operation(
    selected: &mut Selection,
    module: &Module<'_>,
    operation: &OperationDef,
    spec: &UseSpec,
    line: usize,
) -> Result<(), ParseError> {
    let name = &operation.name;
    let alias = spec.alias.as_deref();
    let qualified = &qualified_name(name, alias);
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
        return Err(ParseError::of(
            line,
            LowerError::ImportedFileHasDuplicateCommand {
                name: name.to_string(),
            },
        ));
    }
    let resources = module
        .resources
        .get(name.as_str())
        .map_or(&[][..], Vec::as_slice);
    if !selected.operations.insert(qualified.to_owned()) {
        return Err(ParseError::of(
            line,
            LowerError::DuplicateImport {
                name: name.to_string(),
            },
        ));
    }
    let mut steps = operation.steps.clone();
    for step in &mut steps {
        step.invocation.operation = qualified_name(&step.invocation.operation, alias);
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
    selected
        .pipeline
        .resources
        .extend(resources.iter().map(|line| ResourcesDef {
            operation: qualified.to_owned(),
            resources: line.resources,
        }));
    let checks = operation.inputs.iter().map(|port| &port.checks);
    let checks = checks.chain(operation.outputs.iter().map(|port| &port.checks));
    for used in checks.flatten() {
        import_check(selected, module, &used.check, alias);
    }
    let reviews = operation.inputs.iter().map(|port| &port.reviews);
    let reviews = reviews.chain(operation.outputs.iter().map(|port| &port.reviews));
    for review in reviews.flatten() {
        import_review(selected, module, review, alias);
    }
    for step in &operation.steps {
        for (_, used) in &step.invocation.checks {
            import_check(selected, module, &used.check, alias);
        }
        for used in &step.invocation.reviews {
            if let Some(name) = &used.name {
                import_review(selected, module, name, alias);
            }
        }
    }
    Ok(())
}

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

fn import_source(
    selected: &mut Selection,
    module: &Module<'_>,
    source: &ProductDef,
    qualified: &str,
    alias: Option<&str>,
    line: usize,
) -> Result<(), ParseError> {
    let name = &source.name;
    if !selected.products.insert(qualified.to_owned()) {
        return Err(ParseError::of(
            line,
            LowerError::DuplicateImport {
                name: name.to_string(),
            },
        ));
    }
    selected.pipeline.products.push(ProductDef {
        name: qualified.to_owned(),
        beside: source.beside.as_ref().map(|beside| crate::model::Beside {
            sibling: qualified_name(&beside.sibling, alias),
            ..beside.clone()
        }),
        ..source.clone()
    });
    if source.beside.is_none() {
        if let Some(path) = module.index.path_template_for(name) {
            selected
                .pipeline
                .product_paths
                .insert(qualified.to_owned(), path.with_product(name));
        }
    }
    Ok(())
}

fn import_group(
    selected: &mut Selection,
    module: &Module<'_>,
    group: &SidecarGroup,
    alias: Option<&str>,
    line: usize,
) -> Result<(), ParseError> {
    let qualify = |name: &str| qualified_name(name, alias);
    let mut members = Vec::new();
    for (member, extension) in &group.members {
        let source = module
            .sources
            .get(member.as_str())
            .and_then(|sources| sources.first().copied())
            .expect("companion group members are declared as sources");
        import_source(selected, module, source, &qualify(member), alias, line)?;
        for used in &source.checks {
            import_check(selected, module, &used.check, alias);
        }
        for review in &source.reviews {
            import_review(selected, module, review, alias);
        }
        members.push((qualify(member), extension.clone()));
    }
    selected.pipeline.sidecar_groups.push(SidecarGroup {
        name: qualify(&group.name),
        dimensions: group.dimensions.clone(),
        members,
    });
    Ok(())
}

pub fn parse_pipeline_at(text: &str, path: &Path) -> Result<Pipeline, ParseError> {
    parse_located_document(text, path, Kind::Pipeline).map(|document| document.pipeline)
}

pub(crate) fn parse_located_document(
    text: &str,
    path: &Path,
    kind: Kind,
) -> Result<ParsedDocument, ParseError> {
    match parse_located_document_recovering(text, path, kind) {
        (document, errors) if errors.is_empty() => Ok(document),
        (_, errors) => Err(errors
            .into_iter()
            .next()
            .expect("a document that fails has an error")),
    }
}

pub(crate) fn parse_located_document_recovering(
    text: &str,
    path: &Path,
    kind: Kind,
) -> (ParsedDocument, Vec<ParseError>) {
    let root = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let base = root.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let name = root
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut stack = vec![root.clone()];
    let (document, errors) = parse_document_at_inner(text, &root, &base, &mut stack, kind);
    let errors = errors
        .into_iter()
        .map(|error| named_in(error, &name))
        .collect();
    (document, errors)
}

fn parse_document_at_inner(
    raw: &str,
    path: &Path,
    base: &Path,
    stack: &mut Vec<PathBuf>,
    kind: Kind,
) -> (ParsedDocument, Vec<ParseError>) {
    let text = without_bom(raw);
    let mut files = vec![SourceFile {
        path: path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        blob: fs::read(path)
            .map_or_else(|_| git_blob_id(raw.as_bytes()), |bytes| git_blob_id(&bytes)),
    }];
    let (mut imports, mut file_texts) = (BTreeMap::new(), BTreeMap::new());
    let mut errors = Vec::new();
    let layout = Layout::new(text);
    for (number, line) in layout.numbered() {
        if line.keyword() != Some(Keyword::Use) {
            continue;
        }
        let read = Import {
            number,
            line,
            path,
            base,
        }
        .read(stack, &mut files, &mut file_texts);
        let pipeline = read.unwrap_or_else(|error| {
            errors.push(error);
            Pipeline::default()
        });
        imports.insert(number, pipeline);
    }
    let (mut document, found) = parse_layout(&layout, &imports, kind);
    errors.extend(found);
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    for (root, _) in [&mut document.pipeline.root, &mut document.pipeline.output]
        .into_iter()
        .flatten()
    {
        *root = folder.join(&*root);
    }
    document.pipeline.files = files;
    document.pipeline.folder = fs::canonicalize(base).ok().map(|f| f.display().to_string());
    document.lines.file_texts = file_texts;
    (document, errors)
}

struct Import<'a> {
    number: usize,
    line: &'a Line<'a>,
    path: &'a Path,
    base: &'a Path,
}

impl Import<'_> {
    fn read(
        &self,
        stack: &mut Vec<PathBuf>,
        files: &mut Vec<SourceFile>,
        file_texts: &mut BTreeMap<String, String>,
    ) -> Result<Pipeline, ParseError> {
        let Self {
            number,
            line,
            path,
            base,
        } = *self;
        let spec = parse_use(line.code, number)?;
        let imported_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&spec.path);
        let shown = |path: &Path| relative_path(base, path);
        let canonical = fs::canonicalize(&imported_path).map_err(|error| {
            ParseError::of(
                number,
                LowerError::CannotLoadImport {
                    imported_path: (shown(&imported_path)).to_string(),
                    error: error.to_string(),
                },
            )
        })?;
        if stack.contains(&canonical) {
            return Err(ParseError::of(
                number,
                LowerError::ImportCycleThrough {
                    canonical: (shown(&canonical)).to_string(),
                },
            ));
        }
        if !canonical.is_file() {
            return Err(ParseError::of(
                number,
                LowerError::ImportNotRegularFile {
                    canonical: (shown(&canonical)).to_string(),
                },
            ));
        }
        let imported_text = fs::read_to_string(&canonical).map_err(|error| {
            ParseError::of(
                number,
                LowerError::CannotReadImport {
                    canonical: (shown(&canonical)).to_string(),
                    error: error.to_string(),
                },
            )
        })?;
        stack.push(canonical.clone());
        let (module, errors) =
            parse_document_at_inner(&imported_text, &canonical, base, stack, Kind::Pipeline);
        stack.pop();
        if let Some(first) = errors.first() {
            let at = ImportedAt {
                number,
                use_line: line.text,
                spec_path: &rebase(&spec.path, ""),
                text: &imported_text,
            };
            return Err(in_import(first, at, |file| rebase(&spec.path, file)));
        }
        let rebased = |file: &str| rebase(&spec.path, file);
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
        select_import(&module.pipeline, &spec, number)
    }
}
