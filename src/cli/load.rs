use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use spit::{
    diagnose_checked, diagnose_checked_with_inventory, diagnose_checked_with_records,
    parse_input_spec_at, render_source_inventory, variants, ArtifactReport, Checked, Completeness,
    Context, DimensionOrders, FileNames, InputSource, InputSpec, Likeness, NamedFile, Pipeline,
    Removal, ResolveError, ResolvedInputs, Tool, Variants,
};

use super::args::{CliArgs, Command, Flag};
use super::output::{is_pipeline, is_recipe, passed, read_file, read_stdin, report, Reported};

pub(crate) struct Loaded {
    pub(crate) recipe: InputSpec,
    pipeline_file: PathBuf,
    pipeline_name: Option<String>,
    pub(crate) pipeline_text: String,
    pub(crate) checked: Checked,
    pub(crate) output: Option<PathBuf>,
    pub(crate) invocation: String,
}

impl Loaded {
    pub(crate) fn names(&self) -> FileNames<'_> {
        FileNames {
            pipeline: self.pipeline_name.as_deref(),
            inventory: None,
        }
    }
}

pub(crate) fn load_inputs(args: &CliArgs) -> Result<Loaded, Box<dyn Error>> {
    let file = &args.file;
    let output = args.value(Flag::OutputRoot);
    match args.value(Flag::Root) {
        Some(_) if is_recipe(file) => Err(format!(
            "`{file}` names its dataset with its `root` line; `--root` is for a pipeline run without a recipe"
        )
        .into()),
        root if is_pipeline(file) => {
            load_pipeline(file, root.as_deref(), output.as_deref(), args.command.name())
        }
        _ if output.is_some() && is_recipe(file) => Err(format!(
            "`{file}` names where outputs go with an `output` line; `--output-root` is for a pipeline run without a recipe"
        )
        .into()),
        _ => load_recipe(file),
    }
}

fn load_pipeline(
    file: &str,
    root: Option<&str>,
    output: Option<&str>,
    command: &str,
) -> Result<Loaded, Box<dyn Error>> {
    let pipeline_file = PathBuf::from(file);
    let pipeline_text = read_file(file)?;
    let checked = match diagnose_checked(&pipeline_text, Context::at(&pipeline_file)) {
        Ok(checked) => checked,
        Err(all) => {
            report(&all, &pipeline_text, None, FileNames::default())?;
            return Err(Reported.into());
        }
    };
    if root.is_some() && checked.pipeline.root.is_some() {
        return Err(
            "dataset root is declared in both .spit and `--root`; keep one declaration".into(),
        );
    }
    if output.is_some() && checked.pipeline.output.is_some() {
        return Err(
            "output root is declared in both .spit and `--output-root`; keep one declaration"
                .into(),
        );
    }
    if root.is_none() && checked.pipeline.root.is_none() {
        return Err(format!(
            "`spit {command} {file}` needs to know where the data is: add `--root <directory>`, a `root <directory>` line in the pipeline, or run a .spitin recipe"
        ).into());
    }
    Ok(Loaded {
        recipe: InputSpec {
            pipeline: Some(pipeline_file.clone()),
            root: root.map(|root| (PathBuf::from(root), 0)),
            output: output.map(|output| (PathBuf::from(output), 0)),
            ..InputSpec::default()
        },
        pipeline_name: None,
        pipeline_file,
        pipeline_text,
        output: output.map(PathBuf::from).or_else(|| {
            checked
                .pipeline
                .output
                .as_ref()
                .map(|(output, _)| output.clone())
        }),
        checked,
        invocation: root.map_or_else(|| file.to_owned(), |root| format!("{file} --root {root}")),
    })
}

fn load_recipe(file: &str) -> Result<Loaded, Box<dyn Error>> {
    if !is_recipe(file) {
        return Err(format!(
            "spit inputs reads a .spitin recipe, or a .spit pipeline with a `root` line or `--root`, not `{file}`"
        )
        .into());
    }
    let recipe = parse_input_spec_at(&read_file(file)?, Path::new(file))
        .map_err(|error| format!("{file}: {error}"))?;
    let pipeline_file = recipe.pipeline.clone().ok_or_else(|| {
        format!("{file} does not name its pipeline; add a line such as `pipeline analysis.spit`")
    })?;
    let pipeline_text = read_file(&pipeline_file.display().to_string())?;
    let checked = match diagnose_checked(&pipeline_text, Context::at(&pipeline_file)) {
        Ok(checked) => checked,
        Err(all) => {
            let shown = pipeline_file.display().to_string();
            let names = FileNames {
                pipeline: Some(&shown),
                inventory: None,
            };
            report(&all, &pipeline_text, None, names)?;
            return Err(Reported.into());
        }
    };
    if recipe.root_for(&checked.pipeline)?.is_none() {
        return Err(format!("{file}: a recipe names its dataset root with `root <directory>`, or inherits its pipeline's root").into());
    }
    let output = recipe
        .output_for(&checked.pipeline)?
        .map(|(output, _)| output.clone());
    Ok(Loaded {
        output,
        recipe,
        pipeline_name: Some(pipeline_file.display().to_string()),
        pipeline_file,
        pipeline_text,
        checked,
        invocation: file.to_owned(),
    })
}

pub(crate) fn settle(loaded: &Loaded) -> Result<(ResolvedInputs, PathBuf), Box<dyn Error>> {
    let recipe = &loaded.recipe;
    let (root, _) = recipe
        .root_for(&loaded.checked.pipeline)?
        .cloned()
        .expect("loading checked a root is declared in the pipeline, recipe or command line");
    let source = match &recipe.inventory {
        Some(records) => InputSource::Inventory(records.clone()),
        None => InputSource::Discover(&root),
    };
    let resolved = recipe.resolve(&loaded.checked.pipeline, source)?;
    for skipped in &resolved.skipped {
        eprintln!("warning: skipped {skipped}");
    }
    for incomplete in &resolved.incomplete_groups {
        eprintln!("warning: {incomplete}");
    }
    for missed in &resolved.missed_sources {
        eprintln!("warning: {missed}");
    }
    if let Some(root) = &resolved.root {
        if !resolved.unmatched_files.is_empty() {
            let them = if resolved.unmatched_files.len() == 1 {
                "it"
            } else {
                "them"
            };
            eprintln!(
                "note: {}; `spit inputs {} --unmatched` lists {them}",
                unmatched_files_note(&resolved.unmatched_files, root),
                loaded.invocation
            );
        }
    }
    let pipeline = &loaded.checked.pipeline;
    for note in undeclared_file_notes(pipeline, &root) {
        eprintln!("note: {note}");
    }
    let orders = DimensionOrders::new(pipeline);
    for removal in &resolved.inventory.removed {
        eprintln!("note: {}", removal_note(removal, &orders));
    }
    for likeness in [Likeness::Case, Likeness::Number] {
        for note in variant_notes(&variants(&resolved.inventory, likeness)) {
            eprintln!("note: {note}");
        }
    }
    if let Some(root) = &resolved.root {
        let contexts = if recipe.rules.context_paths.is_empty() {
            String::new()
        } else {
            format!(" and {} contexts", resolved.inventory.contexts.len())
        };
        eprintln!(
            "note: found {} source artifacts{contexts} under `{}`",
            resolved.inventory.artifacts.len(),
            root.display()
        );
    }
    Ok((resolved, root))
}

fn variant_notes(sets: &[Variants]) -> Vec<String> {
    const SHOWN: usize = 3;
    let mut notes = Vec::new();
    let mut start = 0;
    while start < sets.len() {
        let dimension = &sets[start].dimension;
        let end = start
            + sets[start..]
                .iter()
                .take_while(|set| set.dimension == *dimension)
                .count();
        let named: Vec<String> = sets[start..end]
            .iter()
            .take(SHOWN)
            .map(|set| {
                let spellings: Vec<String> = set
                    .spellings
                    .iter()
                    .map(|spelling| {
                        if spelling.products.is_empty() {
                            format!("`{}` (excluded)", spelling.value)
                        } else {
                            format!("`{}` in {}", spelling.value, spelling.products.join(", "))
                        }
                    })
                    .collect();
                spellings.join(", ")
            })
            .collect();
        let more = (end - start).saturating_sub(SHOWN);
        let more = if more > 0 {
            format!("; and {more} more")
        } else {
            String::new()
        };
        let alike = match sets[start].likeness {
            Likeness::Case => "differ only in ASCII letter case",
            Likeness::Number => "are equal as numbers",
        };
        notes.push(format!(
            "`{dimension}` has values that {alike}, which are different values to SPIT: {}{more}",
            named.join("; ")
        ));
        start = end;
    }
    notes
}

fn undeclared_file_notes(pipeline: &Pipeline, root: &Path) -> Vec<String> {
    let declared_in = pipeline.operation_files();
    let here = |operation: &str| -> Option<PathBuf> {
        let folder = Path::new(pipeline.folder.as_deref()?);
        let file = declared_in.get(operation).copied().flatten()?;
        let parent = Path::new(&pipeline.files[file].path).parent()?;
        Some(folder.join(parent))
    };
    let mut found: BTreeSet<(&str, &str, NamedFile<'_>)> = BTreeSet::new();
    for command in &pipeline.commands {
        for line in &command.lines {
            for file in line.named_files() {
                found.insert(("operation", &command.operation, file));
            }
        }
    }
    for check in &pipeline.checks {
        for file in check.template.named_files() {
            found.insert(("check", &check.name, file));
        }
    }
    found
        .into_iter()
        .filter(|&(kind, name, file)| match file {
            NamedFile::Root(path) => root.join(path).is_file(),
            NamedFile::Here(path) => {
                kind == "operation" && here(name).is_some_and(|folder| folder.join(path).is_file())
            }
        })
        .map(|(kind, name, file)| {
            let fix = match file {
                NamedFile::Root(_) => "keep it under the dataset root and pass it to",
                NamedFile::Here(_) => "declare it a `here` source and pass it to",
            };
            format!(
                "{kind} `{name}` runs `{file}`, which it doesn't declare, so editing the file reruns nothing; to rerun on a change, {fix} `{name}` as a source"
            )
        })
        .collect()
}

fn unmatched_files_note(files: &[String], root: &Path) -> String {
    let count = files.len();
    let root = root.display();
    if count == 1 {
        return format!(
            "1 file under `{root}` matches no source rule and is not read: `{}`",
            files[0]
        );
    }
    if count <= 3 {
        let quoted: Vec<_> = files.iter().map(|file| format!("`{file}`")).collect();
        let (last, rest) = quoted.split_last().expect("at least two files");
        return format!(
            "{count} files under `{root}` match no source rule and are not read: {} and {last}",
            rest.join(", ")
        );
    }
    let mut by_extension: BTreeMap<&str, usize> = BTreeMap::new();
    for file in files {
        let name = file.rsplit('/').next().unwrap_or(file);
        let extension = match name.rfind('.') {
            Some(dot) if dot > 0 => &name[dot..],
            _ => "",
        };
        *by_extension.entry(extension).or_default() += 1;
    }
    let mut kinds: Vec<(&str, usize)> = by_extension.into_iter().collect();
    kinds.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    let shown = if kinds.len() > 4 { 3 } else { kinds.len() };
    let mut parts: Vec<String> = kinds[..shown]
        .iter()
        .map(|(extension, n)| match *extension {
            "" => format!("{n} with no extension"),
            extension => format!("{n} `{extension}`"),
        })
        .collect();
    let others: usize = kinds[shown..].iter().map(|(_, n)| n).sum();
    if others > 0 {
        parts.push(format!("{others} others"));
    }
    format!(
        "{count} files under `{root}` match no source rule and are not read ({}), such as `{}`",
        parts.join(", "),
        files[0]
    )
}

pub(crate) fn require_complete(
    inputs: &ResolvedInputs,
    pipeline: &Pipeline,
) -> Result<(), Box<dyn Error>> {
    let Err(failure) = inputs.require_complete() else {
        return Ok(());
    };
    match source_path_failure(inputs, pipeline, &failure) {
        Some(message) => Err(message.into()),
        None => Err(failure.into()),
    }
}

fn source_path_failure(
    inputs: &ResolvedInputs,
    pipeline: &Pipeline,
    failure: &ResolveError,
) -> Option<String> {
    let ResolveError::CoverageViolation {
        product,
        found: 0,
        discovery: false,
        ..
    } = &failure
    else {
        return None;
    };
    let Some(root) = &inputs.root else {
        return None;
    };
    if inputs
        .inventory
        .artifacts
        .iter()
        .any(|record| record.product == *product)
        || !inputs.inventory.removed.is_empty()
        || !inputs.skipped.is_empty()
    {
        return None;
    }
    let mut located = pipeline.clone();
    located
        .product_paths
        .extend(inputs.inventory.source_paths.clone());
    let template = located.path_template_for(product)?;
    let mut message = format!(
        "{failure}\n  no `{product}` files were found under `{}` using path rule `{template}`",
        root.display()
    );
    let near = inputs
        .missed_sources
        .iter()
        .any(|missed| missed.product == *product && missed.nearest.is_some());
    let named = || {
        inputs.unmatched_files.iter().find(|file| {
            file.to_ascii_lowercase()
                .contains(&product.to_ascii_lowercase())
        })
    };
    if near {
        message.push_str(&format!(
            "; the warning above names the nearest file. Check the path template; `path {product}:` sets a rule for this source."
        ));
    } else if let Some(example) = named() {
        message.push_str(&format!(
            "; `{example}` matched no source rule. Check the path template; `path {product}:` sets a rule for this source."
        ));
    }
    Some(message)
}

pub(crate) fn full_path(path: &Path) -> PathBuf {
    if let Ok(found) = fs::canonicalize(path) {
        return found;
    }
    let Ok(absolute) = std::path::absolute(path) else {
        return path.to_path_buf();
    };
    let mut missing = Vec::new();
    let mut current = absolute.as_path();
    while let (Some(name), Some(parent)) = (current.file_name(), current.parent()) {
        missing.push(name);
        if let Ok(found) = fs::canonicalize(parent) {
            return missing
                .iter()
                .rev()
                .fold(found, |path, name| path.join(name));
        }
        current = parent;
    }
    absolute
}

pub(crate) fn recorded_root(root: &Path, file: Option<&Path>) -> PathBuf {
    let root = full_path(root);
    let Some(file) = file else {
        return root;
    };
    let folder = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    relative_to(&root, &full_path(folder)).unwrap_or(root)
}

fn relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    let path: Vec<_> = path.components().collect();
    let base: Vec<_> = base.components().collect();
    let shared = path
        .iter()
        .zip(&base)
        .take_while(|(left, right)| left == right)
        .count();
    if shared == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in shared..base.len() {
        relative.push("..");
    }
    relative.extend(&path[shared..]);
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Some(relative)
}

fn removal_note(removal: &Removal, orders: &DimensionOrders<'_>) -> String {
    let declared = orders.of(removal);
    let identity = removal.identity_in(declared);
    let origin = removal.origin.as_deref().unwrap_or("the recipe");
    let mut note = if removal.is_exclusion() {
        format!("excluded {identity} ({origin})")
    } else {
        format!("excluded {identity} by `{}` ({origin})", removal.rule)
    };
    if let Some(found) = removal.found {
        note.push_str(&format!("; found {found}"));
    }
    if let Some(reason) = &removal.reason {
        note.push_str(&format!(": {reason}"));
    }
    note
}

pub(crate) struct Prepared {
    pub(crate) pipeline: Pipeline,
    pub(crate) inputs: ResolvedInputs,
    pub(crate) report: ArtifactReport,
    pub(crate) root: Option<PathBuf>,
    pub(crate) output: Option<PathBuf>,
    pub(crate) tools: Vec<Tool>,
}

pub(crate) fn prepare(args: &CliArgs) -> Result<Prepared, Box<dyn Error>> {
    let (given, inputs) = match &args.second {
        None => (None, &args.file),
        Some(inputs) => (Some(args.file.as_str()), inputs),
    };
    let command = args.command.name();
    if let (Some(pipeline), true) = (given, is_recipe(inputs)) {
        return Err(format!(
            "`{inputs}` names its own pipeline; run `spit {command} {inputs}` without `{pipeline}`"
        )
        .into());
    }
    if let (Some(_), Some(_)) = (given, args.value(Flag::Root)) {
        return Err(
            "`--root` is for a pipeline run without a recipe or .spitout; a .spitout says where its data is with its `root` line, which `spit inputs -o` writes"
                .into(),
        );
    }
    if let (Some(_), Some(_)) = (given, args.value(Flag::OutputRoot)) {
        return Err(
            "`--output-root` is for a pipeline run without a recipe or .spitout; a .spitout says where outputs go with its `output` line, which `spit inputs -o` writes"
                .into(),
        );
    }
    let completeness = if args.has(Flag::Partial) {
        Completeness::Partial
    } else if args.command == Command::Artifacts {
        Completeness::Reported
    } else {
        Completeness::Required
    };
    if given.is_none() && (is_recipe(inputs) || is_pipeline(inputs)) {
        return prepare_loaded(load_inputs(args)?, completeness);
    }
    let Some(pipeline) = given else {
        return Err(format!(
            "{command} needs a pipeline before `{inputs}`; only a .spitin recipe names its own"
        )
        .into());
    };
    let records_text = if inputs == "-" {
        read_stdin()?
    } else {
        read_file(inputs)?
    };
    let pipeline_text = read_file(pipeline)?;
    let context = Context {
        path: Some(Path::new(pipeline)),
        recipe: None,
        completeness,
    };
    let diagnosis = diagnose_checked_with_records(&pipeline_text, &records_text, context);
    let names = FileNames {
        pipeline: None,
        inventory: Some(if inputs == "-" {
            "standard input"
        } else {
            inputs
        }),
    };
    let (checked, records) = passed(
        diagnosis,
        |(checked, _)| &checked.warnings,
        &pipeline_text,
        Some(&records_text),
        names,
    )?;
    let folder = match inputs.as_str() {
        "-" => Path::new(""),
        file => Path::new(file).parent().unwrap_or_else(|| Path::new("")),
    };
    let root = records
        .inventory
        .root
        .as_ref()
        .map(|recorded| folder.join(recorded));
    let output = match &records.inventory.output {
        Some(recorded) => Some(folder.join(recorded)),
        None => checked
            .pipeline
            .output
            .as_ref()
            .map(|(output, _)| output.clone()),
    };
    let mut inventory = records.inventory;
    for review in &mut inventory.review_files {
        review.file = folder.join(&review.file);
    }
    let settled =
        InputSpec::default().resolve(&checked.pipeline, InputSource::Inventory(inventory))?;
    for incomplete in &settled.incomplete_groups {
        eprintln!("warning: {incomplete}");
    }
    Ok(prepared(
        checked.pipeline,
        settled,
        records.report,
        root,
        output,
    ))
}

fn prepare_loaded(loaded: Loaded, completeness: Completeness) -> Result<Prepared, Box<dyn Error>> {
    let tools = loaded
        .recipe
        .tools
        .as_ref()
        .map(|file| file.tools.clone())
        .unwrap_or_default();
    let mut prepared = prepare_settled(loaded, completeness)?;
    prepared.tools = tools;
    Ok(prepared)
}

fn prepare_settled(loaded: Loaded, completeness: Completeness) -> Result<Prepared, Box<dyn Error>> {
    let (settled, root) = settle(&loaded)?;
    eprintln!("note: ran `spit inputs {}` in memory", loaded.invocation);
    if completeness == Completeness::Required {
        if let Some(failure) = settled.gaps.first().map(|gap| &gap.error) {
            if let Some(message) = source_path_failure(&settled, &loaded.checked.pipeline, failure)
            {
                return Err(message.into());
            }
        }
    }
    let context = Context {
        path: Some(&loaded.pipeline_file),
        recipe: Some(&loaded.recipe),
        completeness,
    };
    let text = &loaded.pipeline_text;
    if let Some((checked, records)) = diagnose_checked_with_inventory(text, &settled, context) {
        report(&checked.warnings, text, None, loaded.names())?;
        let pipeline = loaded.checked.pipeline;
        let output = loaded.output;
        return Ok(prepared(
            pipeline,
            settled,
            records.report,
            Some(root),
            output,
        ));
    }
    let records_text = render_source_inventory(
        &settled.inventory,
        &loaded.checked.pipeline,
        &loaded.recipe.rules,
    );
    let diagnosis = diagnose_checked_with_records(&loaded.pipeline_text, &records_text, context);
    let (_, records) = passed(
        diagnosis,
        |(checked, _)| &checked.warnings,
        &loaded.pipeline_text,
        Some(&records_text),
        loaded.names(),
    )?;
    Ok(prepared(
        loaded.checked.pipeline,
        settled,
        records.report,
        Some(root),
        loaded.output,
    ))
}

fn prepared(
    pipeline: Pipeline,
    inputs: ResolvedInputs,
    mut report: ArtifactReport,
    root: Option<PathBuf>,
    output: Option<PathBuf>,
) -> Prepared {
    report.dag.locate_sources(&inputs.inventory);
    Prepared {
        pipeline,
        inputs,
        report,
        root,
        output,
        tools: Vec::new(),
    }
}
