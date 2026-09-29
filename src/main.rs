use std::env;
use std::error::Error;
use std::fmt::Write;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use spit::{
    diagnose_artifacts_at, diagnose_at, diagnose_at_with_inputs, discover_source_files,
    inspect_paths, parse_document_at, parse_input_spec_at, parse_source_inventory, parse_spit_at,
    parse_spit_without_records_at, render_artifacts, render_bash, render_bound_dag, render_dag,
    render_dag_json, render_source_inventory, resolve, resolve_artifacts_excluding, stage_within,
    validate_concrete_paths, validate_source_files, Diagnostic, Document, InputSource, InputSpec,
    PathCoverage, PathTemplate, Pipeline, ResolvedDag, SourceInventory,
};

#[derive(Clone, Copy, PartialEq)]
enum Command {
    Check,
    Dag,
    Bash,
    Artifacts,
    Discover,
}

const COMMANDS: [Command; 5] = [
    Command::Check,
    Command::Dag,
    Command::Bash,
    Command::Artifacts,
    Command::Discover,
];

impl Command {
    fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Dag => "dag",
            Self::Bash => "bash",
            Self::Artifacts => "artifacts",
            Self::Discover => "discover",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        COMMANDS.into_iter().find(|command| command.name() == name)
    }

    /// The flags this command accepts.
    fn flags(self) -> &'static [Flag] {
        use Flag::*;
        match self {
            Self::Check => &[
                Inputs,
                Sources,
                Root,
                Stage,
                Paths,
                StrictPaths,
                Json,
                Stdin,
            ],
            Self::Dag => &[
                Inputs,
                Sources,
                Root,
                Stage,
                Paths,
                StrictPaths,
                Json,
                Stdin,
            ],
            Self::Bash => &[Inputs, Sources, Root, Stage, StrictPaths, Stdin],
            Self::Artifacts => &[Inputs, Sources, Root, Stdin],
            Self::Discover => &[Inputs, Root, Stdin],
        }
    }

    /// The flags this command cannot run without.
    fn required(self) -> &'static [Flag] {
        &[]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Flag {
    Inputs,
    Sources,
    Root,
    Stage,
    Paths,
    StrictPaths,
    Json,
    Stdin,
}

const FLAGS: [Flag; 8] = [
    Flag::Inputs,
    Flag::Sources,
    Flag::Root,
    Flag::Stage,
    Flag::Paths,
    Flag::StrictPaths,
    Flag::Json,
    Flag::Stdin,
];

/// Pairs of flags that cannot be used together, whatever the command.
const CONFLICTS: [(Flag, Flag); 3] = [
    (Flag::Json, Flag::Paths),
    (Flag::Json, Flag::StrictPaths),
    (Flag::Json, Flag::Stage),
];

impl Flag {
    fn name(self) -> &'static str {
        match self {
            Self::Inputs => "--inputs",
            Self::Sources => "--sources",
            Self::Root => "--root",
            Self::Stage => "--stage",
            Self::Paths => "--paths",
            Self::StrictPaths => "--strict-paths",
            Self::Json => "--json",
            Self::Stdin => "--stdin",
        }
    }

    /// What the flag's value is, for a flag that takes one.
    fn value(self) -> Option<&'static str> {
        match self {
            Self::Inputs => Some("<recipe.spitin>"),
            Self::Sources => Some("<inventory.spitout|->"),
            Self::Root => Some("<directory>"),
            Self::Stage => Some("<name>"),
            Self::Paths | Self::StrictPaths | Self::Json | Self::Stdin => None,
        }
    }

    fn parse(name: &str) -> Option<Self> {
        FLAGS.into_iter().find(|flag| flag.name() == name)
    }
}

/// The flags given on the command line, and each one's value.
#[derive(Default)]
struct Flags(Vec<(Flag, Option<String>)>);

impl Flags {
    fn has(&self, flag: Flag) -> bool {
        self.0.iter().any(|(given, _)| *given == flag)
    }

    fn value(&self, flag: Flag) -> Option<String> {
        self.0
            .iter()
            .find(|(given, _)| *given == flag)
            .and_then(|(_, value)| value.clone())
    }
}

struct CliArgs {
    command: Command,
    pipeline: String,
    inputs: Option<String>,
    sources: Option<String>,
    paths: bool,
    strict_paths: bool,
    root: Option<String>,
    stage: Option<String>,
    json: bool,
    stdin: bool,
}

fn usage() -> String {
    let commands: Vec<_> = COMMANDS.iter().map(|command| command.name()).collect();
    let flags: Vec<_> = FLAGS
        .iter()
        .map(|flag| match flag.value() {
            Some(value) => format!("[{} {value}]", flag.name()),
            None => format!("[{}]", flag.name()),
        })
        .collect();
    format!(
        "usage: spit <{}> <pipeline.spit> {}",
        commands.join("|"),
        flags.join(" ")
    )
}

/// `check, dag, and bash`, for the commands that accept `flag`.
fn commands_accepting(flag: Flag) -> String {
    let names: Vec<_> = COMMANDS
        .iter()
        .filter(|command| command.flags().contains(&flag))
        .map(|command| command.name())
        .collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// A usage error: what is wrong, then the usage line.
fn misuse(problem: impl std::fmt::Display) -> String {
    format!("{problem}\n{}", usage())
}

fn parse_args() -> Result<CliArgs, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let command = match args.next() {
        None => return Err(usage().into()),
        Some(name) => {
            Command::parse(&name).ok_or_else(|| misuse(format_args!("unknown command `{name}`")))?
        }
    };
    let pipeline = args
        .next()
        .ok_or_else(|| misuse(format_args!("{} needs a pipeline file", command.name())))?;
    if pipeline.starts_with("--") {
        return Err(misuse(format_args!(
            "the pipeline file comes before options such as `{pipeline}`"
        ))
        .into());
    }
    let mut flags = Flags::default();
    while let Some(name) = args.next() {
        let flag = Flag::parse(&name).ok_or_else(|| {
            misuse(if name.starts_with('-') {
                format!("unknown option `{name}`")
            } else {
                format!("unexpected argument `{name}`; give one pipeline file")
            })
        })?;
        if flags.has(flag) {
            return Err(misuse(format_args!("{} is given more than once", flag.name())).into());
        }
        let value =
            match flag.value() {
                Some(value) => Some(args.next().ok_or_else(|| {
                    misuse(format_args!("{} needs a value: {value}", flag.name()))
                })?),
                None => None,
            };
        flags.0.push((flag, value));
    }
    check_flags(command, &flags)?;
    Ok(CliArgs {
        command,
        pipeline,
        inputs: flags.value(Flag::Inputs),
        sources: flags.value(Flag::Sources),
        paths: flags.has(Flag::Paths),
        strict_paths: flags.has(Flag::StrictPaths),
        root: flags.value(Flag::Root),
        stage: flags.value(Flag::Stage),
        json: flags.has(Flag::Json),
        stdin: flags.has(Flag::Stdin),
    })
}

/// Check the flags against what `command` accepts and requires, and against
/// each other.
fn check_flags(command: Command, flags: &Flags) -> Result<(), String> {
    for (flag, _) in &flags.0 {
        if !command.flags().contains(flag) {
            return Err(format!(
                "{} applies to {}",
                flag.name(),
                commands_accepting(*flag)
            ));
        }
    }
    for flag in command.required() {
        if !flags.has(*flag) {
            let value = flag
                .value()
                .map_or_else(String::new, |value| format!(" {value}"));
            return Err(format!(
                "{} requires {}{value}",
                command.name(),
                flag.name()
            ));
        }
    }
    for (first, second) in CONFLICTS {
        if flags.has(first)
            && flags.has(second)
            && (command == Command::Check || (command == Command::Dag && second == Flag::Paths))
        {
            return Err(format!(
                "{} cannot be used with {}",
                first.name(),
                second.name()
            ));
        }
    }
    if flags.has(Flag::Stdin) && flags.value(Flag::Sources).as_deref() == Some("-") {
        return Err("--stdin reads the pipeline, so --sources needs a file".into());
    }
    if flags.has(Flag::Inputs) && flags.has(Flag::Sources) {
        return Err("--inputs and --sources select different input descriptions; use one".into());
    }
    Ok(())
}

/// Diagnostics that have already been printed.
#[derive(Debug)]
struct Reported;

impl std::fmt::Display for Reported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("errors reported")
    }
}

impl Error for Reported {}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let diagnostics_json = args.json && args.command == Command::Check;
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        // Editors expect JSON even when the check cannot run.
        Err(error) if diagnostics_json => {
            println!(
                "{{\"diagnostics\":[{{\"severity\":\"error\",\"source\":\"pipeline\",\"line\":null,\"column\":null,\"end_column\":null,\"message\":\"{}\"}}]}}",
                escape_json(&error.to_string())
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            if !error.is::<Reported>() {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: CliArgs) -> Result<(), Box<dyn Error>> {
    let pipeline_text = if args.stdin {
        read_stdin()?
    } else {
        read_file(&args.pipeline)?
    };
    let path = Path::new(&args.pipeline);
    let input_file = input_spec_path(&args, path, &pipeline_text);
    let recipe = input_file
        .as_ref()
        .map(|file| {
            let text = read_file(&file.to_string_lossy())?;
            parse_input_spec_at(&text, file).map_err(|error| format!("{}: {error}", file.display()))
        })
        .transpose()?;
    if args.inputs.is_some() && has_inline_inventory(&pipeline_text, path) {
        return Err(
            "an explicit .spitin recipe cannot be combined with an inline inventory".into(),
        );
    }
    // Step 1 reads the pipeline; the rules and records beside it, and any
    // recipe, are for the input stage. A separate inventory replaces an
    // inline one, which is then not read.
    let parse = |text: &str| {
        if args.sources.is_some() {
            parse_spit_without_records_at(text, path)
        } else {
            parse_spit_at(text, path)
        }
    };
    let document = parse(&pipeline_text).ok();
    let spec = document
        .as_ref()
        .map(|document| input_spec(document, recipe.as_ref()))
        .transpose()?;
    if args.root.is_none()
        && args.sources.is_none()
        && spec.as_ref().is_some_and(|spec| {
            spec.inventory.is_none() && (recipe.is_some() || !spec.rules.discoveries.is_empty())
        })
    {
        args.root = Some(
            input_file
                .as_deref()
                .unwrap_or(path)
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_string_lossy()
                .into_owned(),
        );
    }
    let inventory_text = read_inventory(&args, document.as_ref(), spec.as_ref(), recipe.as_ref())?;
    // Report every error and warning before doing any work.
    let diagnose = if args.command == Command::Artifacts {
        diagnose_artifacts_at
    } else {
        diagnose_at
    };
    let diagnostics = if args.command == Command::Discover && inventory_text.is_some() {
        Vec::new()
    } else if let Some(recipe) = &recipe {
        diagnose_at_with_inputs(
            &pipeline_text,
            inventory_text.as_deref(),
            path,
            recipe,
            args.command == Command::Artifacts,
        )
    } else {
        diagnose(&pipeline_text, inventory_text.as_deref(), path)
    };
    if args.json && args.command == Command::Check {
        print_json(&diagnostics, &pipeline_text, inventory_text.as_deref());
        return Ok(());
    }
    report(&diagnostics, &pipeline_text, inventory_text.as_deref())?;
    if args.command == Command::Discover && args.root.is_none() {
        return Err(
            "discover requires --root unless the pipeline declares directory discovery".into(),
        );
    }
    if args.command == Command::Discover {
        print!("{}", inventory_text.unwrap_or_default());
        return Ok(());
    }
    let document = parse(&pipeline_text)?;
    let spec = input_spec(&document, recipe.as_ref())?;
    let pipeline = document.pipeline;
    // Step 2 settles the records: skip rules, then require rules.
    let records = match &inventory_text {
        Some(text) => Some(parse_source_inventory(text)?),
        None => document.inventory,
    };
    let settled = records
        .map(|records| spec.resolve(&pipeline, InputSource::Inventory(records)))
        .transpose()?;
    // Step 3 sees the logical pipeline and the settled inventory, whose
    // records give each source's file. A pipeline run from a recipe may set
    // no output path; outputs then take the built-in layout.
    let mut bound = pipeline.clone();
    if recipe.is_some() {
        bound
            .path_template
            .get_or_insert_with(PathTemplate::default_output);
    }
    if let Some(stage) = &args.stage {
        check_stage(&pipeline, stage)?;
    }
    let located = settled.iter().flat_map(|settled| {
        settled
            .inventory
            .artifacts
            .iter()
            .filter(|record| record.path.is_some())
            .map(|record| record.product.as_str())
    });
    let coverage = inspect_paths(&bound)?.with_inventory_paths(located);
    if args.command == Command::Check && args.paths {
        println!("{coverage}");
    }
    let Some(settled) = settled else {
        if args.command == Command::Check && args.root.is_none() {
            if args.strict_paths || args.paths {
                coverage.validate(args.strict_paths)?;
            }
            println!("Pipeline valid.\n\nNo source inventory; jobs not resolved.");
            return Ok(());
        }
        return Err("no inline source inventory; supply --sources <inventory.spitout|->".into());
    };
    let inventory = settled.dag_inventory();
    if args.command == Command::Artifacts {
        // `artifacts` shows what the missing requirements hold back.
        let mut report =
            resolve_artifacts_excluding(&pipeline, &inventory, &settled.unavailable())?;
        report.coverage = settled.gaps;
        if let Some(root) = &args.root {
            validate_source_files(&bound, &report.dag, Path::new(root))?;
        }
        print!("{}", render_artifacts(&report));
        return Ok(());
    }
    settled.require_complete()?;
    run_jobs(&args, &pipeline, &bound, &inventory, &coverage)
}

/// The rules and records written beside the pipeline, with a recipe's.
fn input_spec(document: &Document, recipe: Option<&InputSpec>) -> Result<InputSpec, String> {
    let mut spec = InputSpec::embedded_in(document);
    if let Some(recipe) = recipe {
        spec.merge(recipe.clone())?;
    }
    Ok(spec)
}

/// Resolve the jobs of `check`, `dag` or `bash`, check their paths and
/// files as asked, and print the command's output.
fn run_jobs(
    args: &CliArgs,
    pipeline: &Pipeline,
    bound: &Pipeline,
    inventory: &SourceInventory,
    coverage: &PathCoverage,
) -> Result<(), Box<dyn Error>> {
    let dag = resolve(pipeline, inventory)?;
    // A single stage runs on the files earlier stages already wrote.
    let dag = match &args.stage {
        Some(stage) => dag.only_stage(stage),
        None => dag,
    };
    if args.strict_paths || args.paths {
        coverage.validate(args.strict_paths)?;
        validate_concrete_paths(bound, &dag)?;
    }
    let checked_files = args
        .root
        .as_ref()
        .map(|root| validate_source_files(bound, &dag, Path::new(root)))
        .transpose()?;
    match args.command {
        Command::Check => {
            println!(
                "Pipeline valid.\n\n{}",
                job_count(pipeline, &dag, args.stage.as_deref())
            );
            if let Some(verified) = checked_files {
                println!("{verified}");
            }
        }
        Command::Dag if args.json => print!("{}", render_dag_json(bound, &dag)?),
        Command::Dag if args.paths => print!("{}", render_bound_dag(bound, &dag)?),
        Command::Dag => print!("{}", render_dag(&dag)),
        Command::Bash if inventory.artifacts.is_empty() => {
            return Err(
                "the inventory lists no source artifacts, so there is nothing to run".into(),
            )
        }
        Command::Bash => print!("{}", render_bash(bound, &dag)?),
        Command::Artifacts | Command::Discover => unreachable!("handled before resolution"),
    }
    Ok(())
}

/// How many jobs resolved, per stage when the pipeline has stages.
fn job_count(pipeline: &Pipeline, dag: &ResolvedDag, stage: Option<&str>) -> String {
    let total = dag.jobs.len();
    if let Some(stage) = stage {
        return format!("{total} jobs resolved in stage `{stage}`.");
    }
    if pipeline.stages.is_empty() {
        return format!("{total} jobs resolved.");
    }
    // Each outermost stage counts the jobs of the stages nested in it.
    let within = |stage: &str| {
        dag.jobs
            .iter()
            .filter(|job| {
                job.stage
                    .as_deref()
                    .is_some_and(|name| stage_within(name, stage))
            })
            .count()
    };
    let mut parts: Vec<_> = pipeline
        .stages
        .iter()
        .filter(|stage| !stage.name.contains('/'))
        .map(|stage| format!("{} in {}", within(&stage.name), stage.name))
        .collect();
    let outside = dag.jobs.iter().filter(|job| job.stage.is_none()).count();
    if outside > 0 {
        parts.push(format!("{outside} outside stages"));
    }
    format!("{total} jobs resolved: {}.", parts.join(", "))
}

fn read_stdin() -> Result<String, String> {
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|reason| format!("cannot read standard input: {reason}"))?;
    Ok(strip_bom(text))
}

/// Drop a UTF-8 byte order mark, which some Windows editors write.
fn strip_bom(text: String) -> String {
    match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    }
}

/// Read a pipeline or inventory, naming the file if it cannot be read.
fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path)
        .map(strip_bom)
        .map_err(|reason| format!("cannot read `{path}`: {reason}"))
}

/// The inventory text: from `--sources`, a recipe's records, or else found
/// under `--root` when the pipeline has no inline inventory. `None` leaves
/// any inline inventory to be read with the pipeline.
fn read_inventory(
    args: &CliArgs,
    document: Option<&Document>,
    spec: Option<&InputSpec>,
    recipe: Option<&InputSpec>,
) -> Result<Option<String>, Box<dyn Error>> {
    let inline = document.is_some_and(|document| document.inventory.is_some());
    let recipe_records = recipe.and_then(|recipe| recipe.inventory.as_ref());
    Ok(match (args.sources.as_deref(), &args.root) {
        (Some("-"), _) => Some(read_stdin()?),
        (Some(sources), _) => Some(read_file(sources)?),
        (None, Some(root)) if args.command == Command::Discover => document
            .zip(spec)
            .map(|(document, spec)| discover(&document.pipeline, spec, Path::new(root)))
            .transpose()?,
        (None, _) if recipe_records.is_some() => document
            .zip(recipe_records)
            .map(|(document, records)| render_source_inventory(records, &document.pipeline)),
        // With a root and no inventory, find the sources by their path rules.
        (None, Some(root)) if !inline => {
            match document.zip(spec) {
                Some((document, spec)) => {
                    Some(discover(&document.pipeline, spec, Path::new(root))?)
                }
                // The pipeline does not parse; diagnostics report why.
                None => None,
            }
        }
        (None, _) => None,
    })
}

fn input_spec_path(args: &CliArgs, path: &Path, text: &str) -> Option<PathBuf> {
    if let Some(file) = &args.inputs {
        return Some(PathBuf::from(file));
    }
    if args.sources.is_some() || has_inline_inventory(text, path) {
        return None;
    }
    let sibling = path.with_extension("spitin");
    sibling.is_file().then_some(sibling)
}

/// Print every diagnostic, failing if any is an error.
fn report(
    diagnostics: &[Diagnostic],
    pipeline_text: &str,
    inventory_text: Option<&str>,
) -> Result<(), Reported> {
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.display_in(pipeline_text, inventory_text));
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(Reported);
    }
    Ok(())
}

/// `--stage` must name a declared stage.
fn check_stage(pipeline: &Pipeline, stage: &str) -> Result<(), String> {
    if pipeline
        .stages
        .iter()
        .any(|declared| declared.name == stage)
    {
        return Ok(());
    }
    let names: Vec<_> = pipeline
        .stages
        .iter()
        .map(|stage| format!("`{}`", stage.name))
        .collect();
    Err(if names.is_empty() {
        format!("unknown stage `{stage}`; this pipeline declares no stages")
    } else {
        format!("unknown stage `{stage}`; stages: {}", names.join(", "))
    })
}

fn has_inline_inventory(text: &str, path: &Path) -> bool {
    parse_document_at(text, path).is_ok_and(|(_, inventory)| inventory.is_some())
}

/// Scan `root` for the contexts and source files the rules describe, and
/// write them as inventory text.
fn discover(pipeline: &Pipeline, spec: &InputSpec, root: &Path) -> Result<String, Box<dyn Error>> {
    spit::validate_pipeline(pipeline)?;
    spec.check(pipeline)?;
    let discovery = discover_source_files(pipeline, &spec.rules, root)?;
    for skipped in &discovery.skipped {
        eprintln!("warning: skipped {skipped}");
    }
    if spec.rules.discoveries.is_empty() {
        eprintln!(
            "note: discovered {} source artifacts under `{}`",
            discovery.inventory.artifacts.len(),
            root.display()
        );
    } else {
        eprintln!(
            "note: discovered {} source artifacts and {} contexts under `{}`",
            discovery.inventory.artifacts.len(),
            discovery.inventory.contexts.len(),
            root.display()
        );
    }
    Ok(render_source_inventory(&discovery.inventory, pipeline))
}

fn print_json(diagnostics: &[Diagnostic], text: &str, source_text: Option<&str>) {
    let number = |value: Option<usize>| value.map_or_else(|| "null".to_owned(), |n| n.to_string());
    print!("{{\"diagnostics\":[");
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index != 0 {
            print!(",");
        }
        // Columns are 1-based, in UTF-16 code units as editors count them;
        // `end_column` is one past the last character.
        let columns = diagnostic.utf16_columns(text, source_text);
        print!(
            "{{\"severity\":\"{}\",\"source\":\"{}\",\"line\":{},\"column\":{},\"end_column\":{},\"message\":\"{}\"}}",
            diagnostic.severity.as_str(),
            diagnostic.source.as_str(),
            number(diagnostic.line),
            number(columns.as_ref().map(|columns| columns.start + 1)),
            number(columns.as_ref().map(|columns| columns.end + 1)),
            escape_json(&diagnostic.message)
        );
    }
    println!("]}}");
}

fn escape_json(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c < ' ' => write!(escaped, "\\u{:04x}", u32::from(c)).unwrap(),
            c => escaped.push(c),
        }
    }
    escaped
}
