//! Reading the files a command names, and running the steps before its own
//! in memory: settling a recipe's inputs, and preparing a pipeline and
//! inventory for `dag` and `artifacts`.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use spit::{
    diagnose_checked, diagnose_checked_with_inventory, diagnose_checked_with_records,
    parse_input_spec_at, render_source_inventory, ArtifactReport, Checked, Context, FileNames,
    InputSource, InputSpec, Pipeline, Removal, ResolveError, ResolvedInputs,
};

use super::args::{CliArgs, Command, Flag};
use super::output::{is_pipeline, is_recipe, passed, read_file, read_stdin, report, Reported};

/// A recipe, and the pipeline its `pipeline` line names, checked; or a
/// pipeline given with `--root`, as a recipe with no rules.
pub(crate) struct Loaded {
    pub(crate) recipe: InputSpec,
    /// The file the recipe's `pipeline` line names.
    pipeline_file: PathBuf,
    /// That file as messages name it, when the recipe named it rather than
    /// the command line.
    pipeline_name: Option<String>,
    pub(crate) pipeline_text: String,
    pub(crate) checked: Checked,
    /// What `spit inputs` takes to run step 2 again: the recipe, or the
    /// pipeline and its root.
    pub(crate) invocation: String,
}

impl Loaded {
    /// Names for messages about the pipeline, which the user did not give
    /// on the command line when a recipe names it.
    pub(crate) fn names(&self) -> FileNames<'_> {
        FileNames {
            pipeline: self.pipeline_name.as_deref(),
            inventory: None,
        }
    }
}

/// Read what step 2 starts from: a recipe, or a pipeline with the dataset
/// folder `--root` gives, which only a pipeline given alone takes.
pub(crate) fn load_inputs(args: &CliArgs) -> Result<Loaded, Box<dyn Error>> {
    let file = &args.file;
    let command = args.command.name();
    match args.value(Flag::Root) {
        Some(_) if is_recipe(file) => Err(format!(
            "`{file}` names its dataset with its `root` line; `--root` is for a pipeline run without a recipe"
        )
        .into()),
        Some(root) if is_pipeline(file) => load_pipeline(file, &root),
        None if is_pipeline(file) => Err(format!(
            "`spit {command} {file}` needs to know where the data is: add `--root <directory>`, or run a .spitin recipe"
        )
        .into()),
        _ => load_recipe(file),
    }
}

/// Check the pipeline `file`, to scan `root` with its own path rules and
/// no recipe.
fn load_pipeline(file: &str, root: &str) -> Result<Loaded, Box<dyn Error>> {
    let pipeline_file = PathBuf::from(file);
    let pipeline_text = read_file(file)?;
    let checked = match diagnose_checked(&pipeline_text, Context::at(&pipeline_file)) {
        Ok(checked) => checked,
        Err(all) => {
            report(&all, &pipeline_text, None, FileNames::default())?;
            return Err(Reported.into());
        }
    };
    Ok(Loaded {
        recipe: InputSpec {
            pipeline: Some(pipeline_file.clone()),
            root: Some((PathBuf::from(root), 0)),
            ..InputSpec::default()
        },
        pipeline_name: None,
        pipeline_file,
        pipeline_text,
        checked,
        invocation: format!("{file} --root {root}"),
    })
}

/// Read the recipe `file` and check the pipeline it names, printing the
/// pipeline's diagnostics only when it fails; its warnings are left to the
/// caller.
fn load_recipe(file: &str) -> Result<Loaded, Box<dyn Error>> {
    if !is_recipe(file) {
        return Err(format!(
            "spit inputs reads a .spitin recipe, or a .spit pipeline with `--root`, not `{file}`"
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
    Ok(Loaded {
        recipe,
        pipeline_name: Some(pipeline_file.display().to_string()),
        pipeline_file,
        pipeline_text,
        checked,
        invocation: file.to_owned(),
    })
}

/// Run step 2: scan the root the recipe's `root` line or `--root` names,
/// or take the records written in the recipe. Returns what it settled, and
/// the dataset root.
pub(crate) fn settle(loaded: &Loaded) -> Result<(ResolvedInputs, PathBuf), Box<dyn Error>> {
    let recipe = &loaded.recipe;
    let (root, _) = recipe
        .root
        .clone()
        .expect("a recipe read from a file names its root, and `--root` gives a pipeline's");
    // Records written in the recipe stand in for a scan. The `root` line
    // still says where their files are.
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
        let count = resolved.unmatched_files.len();
        if count > 0 {
            // An example says what kind of file is left out, which is
            // usually enough to see that leaving it out is right.
            let example = &resolved.unmatched_files[0];
            eprintln!("note: {count} files under `{}` match no source rule and are not read, such as `{example}`; `spit inputs {} --unmatched` lists them", root.display(), loaded.invocation);
        }
    }
    let pipeline = &loaded.checked.pipeline;
    for removal in &resolved.inventory.removed {
        eprintln!("note: {}", removal_note(removal, pipeline));
    }
    if let Some(root) = &resolved.root {
        let contexts = if recipe.rules.discoveries.is_empty() {
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

/// Explain a missing scanned source with the path rule the scan used. A
/// coverage error alone says how many artifacts were found, but not why a
/// file visible under the root may have been left out.
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
    // The warning `settle` printed names the file nearest the rule. A rule
    // no file comes near may still be the wrong rule for a file named after
    // the source.
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

/// The dataset `root` as a `.spitout` written to `file` records it: relative
/// to the file's folder.
pub(crate) fn recorded_root(root: &Path, file: &Path) -> PathBuf {
    let full = |path: &Path| {
        fs::canonicalize(path)
            .or_else(|_| std::path::absolute(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let root = full(root);
    let folder = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    relative_to(&root, &full(folder)).unwrap_or(root)
}

/// `path` relative to the folder `base`, both absolute, or `None` when they
/// share no root, as on two Windows drives.
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

/// What an `exclude` or `drop` rule removed, as a note says it:
/// `excluded bold[sub=02,run=3] (line 4): corrupted`, or `dropped [sub=03]
/// by \`drop [sub] where sessions count<2\` (line 6); found 1`.
fn removal_note(removal: &Removal, pipeline: &Pipeline) -> String {
    // An artifact's dimensions in its product's order; a group's in the
    // order the pipeline first declares them, as the .spitout writes it.
    let declared: Vec<String> = match &removal.product {
        Some(name) => pipeline
            .products
            .iter()
            .find(|product| &product.name == name)
            .map(|product| product.dimensions.clone())
            .unwrap_or_default(),
        None => {
            let mut order: Vec<String> = Vec::new();
            for dimension in pipeline
                .products
                .iter()
                .flat_map(|product| &product.dimensions)
            {
                if !order.contains(dimension) {
                    order.push(dimension.clone());
                }
            }
            order
        }
    };
    let identity = removal.identity_in(&declared);
    let origin = removal.origin.as_deref().unwrap_or("the recipe");
    let mut note = if removal.is_exclusion() {
        format!("excluded {identity} ({origin})")
    } else {
        format!("dropped {identity} by `{}` ({origin})", removal.rule)
    };
    if let Some(found) = removal.found {
        note.push_str(&format!("; found {found}"));
    }
    if let Some(reason) = &removal.reason {
        note.push_str(&format!(": {reason}"));
    }
    note
}

/// A pipeline ready for step 3: its settled inputs, and the pipeline used
/// to bind paths.
pub(crate) struct Prepared {
    pub(crate) pipeline: Pipeline,
    pub(crate) inputs: ResolvedInputs,
    /// What the inputs resolve to, from their diagnosis; its sources get
    /// their files from `inputs` in [`prepared`].
    pub(crate) report: ArtifactReport,
    /// Where source files are, when known.
    pub(crate) root: Option<PathBuf>,
}

/// Read the pipeline and its inputs for step 3, running step 2 in memory
/// for a recipe, and report every diagnostic first.
pub(crate) fn prepare(args: &CliArgs) -> Result<Prepared, Box<dyn Error>> {
    // A recipe names its own pipeline, so it stands alone; any other inputs
    // need the pipeline they are for.
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
    let lenient = args.command == Command::Artifacts || args.has(Flag::Partial);
    if given.is_none() && (is_recipe(inputs) || is_pipeline(inputs)) {
        return prepare_loaded(load_inputs(args)?, lenient);
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
        lenient,
    };
    let diagnosis = diagnose_checked_with_records(&pipeline_text, &records_text, context);
    // The pipeline is the file given first; the records are named, as they
    // may be in another file or read from standard input.
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
    // A root the records name is relative to their file's folder.
    let root = records.inventory.root.as_ref().map(|recorded| {
        let folder = match inputs.as_str() {
            "-" => Path::new(""),
            file => Path::new(file).parent().unwrap_or_else(|| Path::new("")),
        };
        folder.join(recorded)
    });
    let settled = InputSpec::default()
        .resolve(&checked.pipeline, InputSource::Inventory(records.inventory))?;
    Ok(prepared(checked.pipeline, settled, records.report, root))
}

/// Step 2 in memory for the recipe `file`, then step 3's diagnosis of the
/// records it settles. The pipeline is read and settled once, and its
/// warnings are printed once, with the records'.
fn prepare_loaded(loaded: Loaded, lenient: bool) -> Result<Prepared, Box<dyn Error>> {
    let (settled, root) = settle(&loaded)?;
    eprintln!("note: ran `spit inputs {}` in memory", loaded.invocation);
    if !lenient {
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
        lenient,
    };
    // The records are written as a .spitout only when a diagnostic needs
    // lines of it to point at.
    let text = &loaded.pipeline_text;
    if let Some((checked, records)) = diagnose_checked_with_inventory(text, &settled, context) {
        report(&checked.warnings, text, None, loaded.names())?;
        let pipeline = loaded.checked.pipeline;
        return Ok(prepared(pipeline, settled, records.report, Some(root)));
    }
    let records_text = render_source_inventory(
        &settled.inventory,
        &loaded.checked.pipeline,
        &loaded.recipe.rules,
    );
    let diagnosis = diagnose_checked_with_records(&loaded.pipeline_text, &records_text, context);
    // The records were settled in memory, so they have no file to name.
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
    ))
}

/// `pipeline` ready for step 3 with its `inputs`.
fn prepared(
    pipeline: Pipeline,
    inputs: ResolvedInputs,
    mut report: ArtifactReport,
    root: Option<PathBuf>,
) -> Prepared {
    report.dag.locate_sources(&inputs.inventory);
    Prepared {
        pipeline,
        inputs,
        report,
        root,
    }
}
