//! The `spit` command line. Each command is one step, and the files given
//! say what it works on:
//!
//! 1. `check` compiles a pipeline, or checks a recipe against its pipeline;
//! 2. `inputs` settles a dataset from a recipe, writing a `.spitout`;
//! 3. `dag` and `artifacts` resolve a pipeline's jobs over a `.spitout`.
//!
//! A command given files from an earlier step runs the steps between in
//! memory. Nothing is loaded that the command line does not name.

/// SPIT makes and frees many small strings; mimalloc does this faster than
/// the system allocator.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use spit::{
    bind_dag, bind_dag_with, diagnose_checked, diagnose_checked_with_inventory,
    diagnose_checked_with_records, diagnose_recipe, inspect_paths, parse_input_spec_at,
    render_artifacts, render_bound_dag, render_dag, render_diagnostics_json, render_editor_json,
    render_source_inventory, stage_within, validate_bound_source_files, validate_source_files,
    ArtifactReport, BoundDag, BoundPaths, Checked, Context, Diagnosis, Diagnostic,
    DiagnosticSource, InputSource, InputSpec, PathTemplate, Pipeline, ResolvedDag, ResolvedInputs,
    Severity,
};

#[derive(Clone, Copy, PartialEq)]
enum Command {
    Check,
    Inputs,
    Dag,
    Artifacts,
}

const COMMANDS: [Command; 4] = [
    Command::Check,
    Command::Inputs,
    Command::Dag,
    Command::Artifacts,
];

struct CommandSpec {
    name: &'static str,
    files: &'static str,
    summary: &'static str,
    example: &'static str,
    flags: &'static [Flag],
}

impl Command {
    fn spec(self) -> CommandSpec {
        use Flag::{Hovers, Json, Output, PathRules, Paths, Root, Stdin, StrictPaths};
        match self {
            Self::Check => CommandSpec {
                name: "check",
                files: "<pipeline.spit | recipe.spitin>",
                summary: "step 1: compile a pipeline, or check a recipe against its pipeline; reads no data",
                example: "spit check analysis.spit\n  spit check dataset.spitin",
                flags: &[PathRules, StrictPaths, Json, Stdin, Hovers],
            },
            Self::Inputs => CommandSpec {
                name: "inputs",
                files: "<recipe.spitin>",
                summary: "step 2: find a dataset's sources with a recipe, apply `skip` and `require`, and write a .spitout",
                example: "spit inputs dataset.spitin -o dataset.spitout",
                flags: &[Root, Output],
            },
            Self::Dag => CommandSpec {
                name: "dag",
                files: "<recipe.spitin> or <pipeline.spit> <inputs.spitout | ->",
                summary: "step 3: resolve a pipeline's jobs over a dataset's inputs; -o writes the .spitdag",
                example: "spit dag dataset.spitin -o analysis.spitdag\n  spit dag analysis.spit dataset.spitout -o analysis.spitdag\n  spit dag analysis.spit dataset.spitout --paths",
                flags: &[Root, StrictPaths, Paths, Json, Output],
            },
            Self::Artifacts => CommandSpec {
                name: "artifacts",
                files: "<recipe.spitin> or <pipeline.spit> <inputs.spitout | ->",
                summary: "step 3: report what can and cannot be made from a dataset's inputs, and why",
                example: "spit artifacts dataset.spitin\n  spit artifacts analysis.spit dataset.spitout",
                flags: &[Root],
            },
        }
    }

    fn name(self) -> &'static str {
        self.spec().name
    }

    fn parse(name: &str) -> Option<Self> {
        COMMANDS.into_iter().find(|command| command.name() == name)
    }

    /// The files it takes, as the usage line shows them.
    fn files(self) -> &'static str {
        self.spec().files
    }

    /// The most files it takes; every command takes at least one.
    fn most_files(self) -> usize {
        match self {
            Self::Check | Self::Inputs => 1,
            Self::Dag | Self::Artifacts => 2,
        }
    }

    fn summary(self) -> &'static str {
        self.spec().summary
    }

    fn example(self) -> &'static str {
        self.spec().example
    }

    /// Given a recipe in place of a `.spitout`, or earlier files in place of
    /// a `.spitdag`, the command runs the steps between in memory.
    fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Dag | Self::Artifacts => Some(
                "Given a .spitin in place of the .spitout, it runs `spit inputs` in memory first.\nA .spitin names its own pipeline, so it is given alone; a .spitout or `-` needs the pipeline first.",
            ),
            Self::Check | Self::Inputs => None,
        }
    }

    /// The flags this command accepts.
    fn flags(self) -> &'static [Flag] {
        self.spec().flags
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Flag {
    Root,
    Output,
    Paths,
    PathRules,
    StrictPaths,
    Json,
    Stdin,
    Hovers,
}

const FLAGS: [Flag; 8] = [
    Flag::Root,
    Flag::Output,
    Flag::Paths,
    Flag::PathRules,
    Flag::StrictPaths,
    Flag::Json,
    Flag::Stdin,
    Flag::Hovers,
];

/// Pairs of flags that cannot be used together.
const CONFLICTS: [(Flag, Flag); 5] = [
    (Flag::Json, Flag::Paths),
    (Flag::Json, Flag::Output),
    (Flag::Paths, Flag::Output),
    (Flag::Json, Flag::PathRules),
    (Flag::Json, Flag::StrictPaths),
];

impl Flag {
    fn name(self) -> &'static str {
        match self {
            Self::Root => "--root",
            Self::Output => "-o",
            Self::Paths => "--paths",
            Self::PathRules => "--path-rules",
            Self::StrictPaths => "--strict-paths",
            Self::Json => "--json",
            Self::Stdin => "--stdin",
            Self::Hovers => "--hovers",
        }
    }

    /// What the flag's value is, for a flag that takes one.
    fn value(self) -> Option<&'static str> {
        match self {
            Self::Root => Some("<directory>"),
            Self::Output => Some("<file>"),
            _ => None,
        }
    }

    fn help(self, command: Command) -> &'static str {
        match (self, command) {
            (Self::Root, Command::Inputs) => "the folder to scan; the recipe's folder by default",
            (Self::Root, _) => "the dataset folder, to check that each source file exists",
            (Self::Output, Command::Inputs) => "write the .spitout to <file>, not standard output",
            (Self::Output, _) => "write the .spitdag to <file>",
            (Self::Paths, _) => "show each artifact's file",
            (Self::PathRules, _) => "list the path rule each product uses",
            (Self::StrictPaths, _) => "require an explicit path rule for every product",
            (Self::Json, Command::Check) => "print diagnostics as JSON, for editors",
            (Self::Json, _) => "print the .spitdag",
            (Self::Stdin, _) => {
                "read the file's text from standard input; the file names its location"
            }
            (Self::Hovers, _) => {
                "include operation and product hovers with --json (pipelines only)"
            }
        }
    }

    fn parse(name: &str) -> Option<Self> {
        if name == "--output" {
            return Some(Self::Output);
        }
        FLAGS.into_iter().find(|flag| flag.name() == name)
    }
}

/// The flags given on the command line, and each one's value.
#[derive(Default)]
struct Flags(Vec<(Flag, Option<String>)>);

impl Flags {
    /// Add the flag `argument` for `command`, taking its value from `rest`
    /// when it has one.
    fn add(
        &mut self,
        command: Command,
        argument: &str,
        rest: &mut impl Iterator<Item = String>,
    ) -> Result<(), String> {
        let flag = Flag::parse(argument)
            .ok_or_else(|| misuse(format_args!("unknown option `{argument}`"), Some(command)))?;
        if !command.flags().contains(&flag) {
            let accepting: Vec<_> = COMMANDS
                .iter()
                .filter(|other| other.flags().contains(&flag))
                .map(|other| other.name())
                .collect();
            return Err(misuse(
                format_args!("{} applies to {}", flag.name(), accepting.join(", ")),
                Some(command),
            ));
        }
        if self.has(flag) {
            return Err(misuse(
                format_args!("{} is given more than once", flag.name()),
                Some(command),
            ));
        }
        let value = match flag.value() {
            Some(value) => Some(rest.next().ok_or_else(|| {
                misuse(
                    format_args!("{} needs a value: {value}", flag.name()),
                    Some(command),
                )
            })?),
            None => None,
        };
        self.0.push((flag, value));
        Ok(())
    }

    /// Fail if two flags that cannot be used together were both given.
    fn check_conflicts(&self, command: Command) -> Result<(), String> {
        if self.has(Flag::Hovers) && !self.has(Flag::Json) {
            return Err(misuse("--hovers requires --json", Some(command)));
        }
        for (first, second) in CONFLICTS {
            if self.has(first) && self.has(second) {
                return Err(misuse(
                    format_args!("{} cannot be used with {}", first.name(), second.name()),
                    Some(command),
                ));
            }
        }
        Ok(())
    }

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
    /// The file every command takes.
    file: String,
    /// The inputs after a pipeline, for a command that takes two files.
    second: Option<String>,
    flags: Flags,
}

impl CliArgs {
    fn has(&self, flag: Flag) -> bool {
        self.flags.has(flag)
    }

    fn value(&self, flag: Flag) -> Option<String> {
        self.flags.value(flag)
    }
}

/// What the command line asks for.
enum Request {
    Run(CliArgs),
    Help(Option<Command>),
    Version,
}

/// `spit help`, or `spit help <command>`.
struct Help(Option<Command>);

impl fmt::Display for Help {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            None => overview(f),
            Some(command) => command_help(f, command),
        }
    }
}

fn overview(f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(
        "spit: compile a pipeline, settle a dataset's inputs, resolve jobs, and write a script\n\nusage: spit <command> <files> [options]\n\ncommands:\n",
    )?;
    for command in COMMANDS {
        writeln!(f, "  {:<10} {}", command.name(), command.summary())?;
    }
    f.write_str(
        "\nfiles:\n  .spit      a pipeline: sources, operations, steps, commands, path rules\n  .spitin    a recipe for a dataset's inputs, naming its pipeline\n  .spitout   a dataset's settled inputs, each source with its file\n  .spitdag   the resolved jobs, each with its files and command\n\nRun `spit help <command>` for its options.\n",
    )
}

fn command_help(f: &mut fmt::Formatter<'_>, command: Command) -> fmt::Result {
    let name = command.name();
    writeln!(f, "spit {name}: {}\n", command.summary())?;
    writeln!(f, "usage: spit {name} {} [options]", command.files())?;
    if let Some(shortcut) = command.shortcut() {
        writeln!(f, "\n{shortcut}")?;
    }
    if !command.flags().is_empty() {
        writeln!(f, "\noptions:")?;
        for flag in command.flags() {
            let name = match flag.value() {
                Some(value) => format!("{} {value}", flag.name()),
                None => flag.name().to_owned(),
            };
            writeln!(f, "  {name:<20} {}", flag.help(command))?;
        }
    }
    writeln!(f, "\nexample:\n  {}", command.example())
}

/// A usage error: what is wrong, and where to read more.
fn misuse(problem: impl std::fmt::Display, command: Option<Command>) -> String {
    let more = command.map_or_else(
        || "run `spit help`".to_owned(),
        |command| {
            format!(
                "usage: spit {} {} [options]; run `spit help {}`",
                command.name(),
                command.files(),
                command.name()
            )
        },
    );
    format!("{problem}\n{more}")
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Request, String> {
    let mut args = args.into_iter();
    let command = match args.next().as_deref() {
        None | Some("help" | "--help" | "-h") => {
            return match args.next() {
                None => Ok(Request::Help(None)),
                Some(name) => Command::parse(&name)
                    .map(|command| Request::Help(Some(command)))
                    .ok_or_else(|| misuse(format_args!("unknown command `{name}`"), None)),
            };
        }
        Some("--version" | "-V") => return Ok(Request::Version),
        Some(name) => Command::parse(name)
            .ok_or_else(|| misuse(format_args!("unknown command `{name}`"), None))?,
    };
    let mut files = Vec::new();
    let mut flags = Flags::default();
    while let Some(argument) = args.next() {
        if matches!(argument.as_str(), "--help" | "-h") {
            return Ok(Request::Help(Some(command)));
        }
        if argument == "-" || !argument.starts_with('-') {
            files.push(argument);
            continue;
        }
        flags.add(command, &argument, &mut args)?;
    }
    flags.check_conflicts(command)?;
    let (file, second) = take_files(command, files)?;
    Ok(Request::Run(CliArgs {
        command,
        file,
        second,
        flags,
    }))
}

/// The file `command` takes, and a second when it takes two.
fn take_files(command: Command, files: Vec<String>) -> Result<(String, Option<String>), String> {
    let mut files = files.into_iter();
    let Some(file) = files.next() else {
        return Err(misuse(
            format_args!("{} needs {}", command.name(), command.files()),
            Some(command),
        ));
    };
    let second = files.next();
    let extra = if command.most_files() == 1 {
        second.as_ref()
    } else {
        files.as_slice().first()
    };
    if let Some(extra) = extra {
        return Err(misuse(
            format_args!("unexpected file `{extra}`"),
            Some(command),
        ));
    }
    Ok((file, second))
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
    let args = match parse_args(env::args().skip(1)) {
        Ok(Request::Run(args)) => args,
        Ok(Request::Help(command)) => {
            print!("{}", Help(command));
            return ExitCode::SUCCESS;
        }
        Ok(Request::Version) => {
            println!("spit {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let diagnostics_json = args.has(Flag::Json) && args.command == Command::Check;
    let result = match args.command {
        Command::Check => check(&args),
        Command::Inputs => inputs(&args),
        Command::Dag => dag(&args),
        Command::Artifacts => artifacts(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // Editors expect JSON even when the check cannot run.
        Err(error) if diagnostics_json => {
            let diagnostic = Diagnostic {
                severity: Severity::Error,
                source: DiagnosticSource::Pipeline,
                line: None,
                columns: None,
                message: error.to_string(),
            };
            print!("{}", render_diagnostics_json(&[diagnostic], "", None));
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

/// Step 1: compile a pipeline, or check a recipe against the pipeline it
/// names. Reads no data.
fn check(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let file = &args.file;
    let path = Path::new(file);
    let text = if args.has(Flag::Stdin) {
        read_stdin()?
    } else {
        read_file(file)?
    };
    if is_recipe(file) {
        if args.has(Flag::Hovers) {
            return Err("--hovers describes a .spit pipeline, not a recipe".into());
        }
        if args.has(Flag::PathRules) || args.has(Flag::StrictPaths) {
            return Err("--path-rules and --strict-paths check a pipeline, not a recipe".into());
        }
        let diagnostics = diagnose_recipe(&text, path);
        if args.has(Flag::Json) {
            print!("{}", render_diagnostics_json(&diagnostics, &text, None));
            return Ok(());
        }
        report(&diagnostics, &text, None)?;
        println!("Recipe valid.");
        return Ok(());
    }
    let diagnosis = diagnose_checked(&text, Context::at(path));
    if args.has(Flag::Json) {
        let diagnostics = match &diagnosis {
            Ok(checked) => &checked.warnings,
            Err(all) => all,
        };
        if args.has(Flag::Hovers) {
            print!("{}", render_editor_json(diagnostics, &text, path));
        } else {
            print!("{}", render_diagnostics_json(diagnostics, &text, None));
        }
        return Ok(());
    }
    let checked = passed(diagnosis, |checked| &checked.warnings, &text, None)?;
    let coverage = inspect_paths(&checked.pipeline)?;
    if args.has(Flag::PathRules) {
        println!("{coverage}");
    }
    if args.has(Flag::StrictPaths) {
        coverage.validate(true)?;
    }
    println!("Pipeline valid.");
    Ok(())
}

/// Step 2: settle a dataset from a recipe and write its `.spitout`.
fn inputs(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let loaded = load_recipe(&args.file)?;
    report(&loaded.checked.warnings, &loaded.pipeline_text, None)?;
    let root = args.value(Flag::Root).map(PathBuf::from);
    let settled = settle(&loaded, &args.file, root.as_deref())?;
    settled.require_complete()?;
    let text = render_source_inventory(
        &settled.inventory,
        &loaded.checked.pipeline,
        &loaded.recipe.rules,
    );
    write_output(args, &text, "the .spitout")
}

/// A recipe, and the pipeline its `pipeline` line names, checked.
struct Loaded {
    recipe: InputSpec,
    /// The file the recipe's `pipeline` line names.
    pipeline_file: PathBuf,
    pipeline_text: String,
    checked: Checked,
}

/// Read the recipe `file` and check the pipeline it names, printing the
/// pipeline's diagnostics only when it fails; its warnings are left to the
/// caller.
fn load_recipe(file: &str) -> Result<Loaded, Box<dyn Error>> {
    if !is_recipe(file) {
        return Err(format!("spit inputs reads a .spitin recipe, not `{file}`").into());
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
            report(&all, &pipeline_text, None)?;
            return Err(Reported.into());
        }
    };
    Ok(Loaded {
        recipe,
        pipeline_file,
        pipeline_text,
        checked,
    })
}

/// Run step 2 for the recipe `file`: scan `root`, or the recipe's folder,
/// or take the records written in the recipe when no root is given.
fn settle(
    loaded: &Loaded,
    file: &str,
    root: Option<&Path>,
) -> Result<ResolvedInputs, Box<dyn Error>> {
    let folder = Path::new(file)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_owned();
    let scan = root.is_some();
    let root = root.map_or(folder, Path::to_path_buf);
    let recipe = &loaded.recipe;
    // Records written in the recipe stand in for a scan, unless a root to
    // scan is given.
    let source = match &recipe.inventory {
        Some(records) if !scan => InputSource::Inventory(records.clone()),
        _ => InputSource::Discover(&root),
    };
    let resolved = recipe.resolve(&loaded.checked.pipeline, source)?;
    for skipped in &resolved.skipped {
        eprintln!("warning: skipped {skipped}");
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
    Ok(resolved)
}

/// A pipeline ready for step 3: its settled inputs, and the pipeline used
/// to bind paths.
struct Prepared {
    pipeline: Pipeline,
    bound: Pipeline,
    inputs: ResolvedInputs,
    /// What the inputs resolve to, from their diagnosis; its sources get
    /// their files from `inputs` in [`prepared`].
    report: ArtifactReport,
    /// Where source files are, when known.
    root: Option<PathBuf>,
}

/// Read the pipeline and its inputs for step 3, running step 2 in memory
/// for a recipe, and report every diagnostic first.
fn prepare(args: &CliArgs) -> Result<Prepared, Box<dyn Error>> {
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
    let lenient = args.command == Command::Artifacts;
    let root = args.value(Flag::Root).map(PathBuf::from);
    if is_recipe(inputs) {
        return prepare_recipe(inputs, root, lenient);
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
    let (checked, records) = passed(
        diagnosis,
        |(checked, _)| &checked.warnings,
        &pipeline_text,
        Some(&records_text),
    )?;
    let settled = InputSpec::default()
        .resolve(&checked.pipeline, InputSource::Inventory(records.inventory))?;
    Ok(prepared(checked.pipeline, settled, records.report, root))
}

/// Step 2 in memory for the recipe `file`, then step 3's diagnosis of the
/// records it settles. The pipeline is read and settled once, and its
/// warnings are printed once, with the records'.
fn prepare_recipe(
    file: &str,
    root: Option<PathBuf>,
    lenient: bool,
) -> Result<Prepared, Box<dyn Error>> {
    let loaded = load_recipe(file)?;
    let settled = settle(&loaded, file, root.as_deref())?;
    eprintln!("note: ran `spit inputs {file}` in memory");
    let context = Context {
        path: Some(&loaded.pipeline_file),
        recipe: Some(&loaded.recipe),
        lenient,
    };
    let root = root.or_else(|| settled.root.clone());
    // The records are written as a .spitout only when a diagnostic needs
    // lines of it to point at.
    let text = &loaded.pipeline_text;
    if let Some((checked, records)) = diagnose_checked_with_inventory(text, &settled, context) {
        report(&checked.warnings, text, None)?;
        let pipeline = loaded.checked.pipeline;
        return Ok(prepared(pipeline, settled, records.report, root));
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
    )?;
    Ok(prepared(
        loaded.checked.pipeline,
        settled,
        records.report,
        root,
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
    // Records give every source its file; outputs with no rule take the
    // built-in layout.
    let mut bound = pipeline.clone();
    bound
        .path_template
        .get_or_insert_with(PathTemplate::default_output);
    Prepared {
        pipeline,
        bound,
        inputs,
        report,
        root,
    }
}

/// Step 3: resolve the jobs and print them, or write the `.spitdag`.
fn dag(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(args)?;
    prepared.inputs.require_complete()?;
    let dag = &prepared.report.dag;
    if args.has(Flag::StrictPaths) {
        inspect_paths(&prepared.bound)?
            .with_inventory_paths(located(&prepared.inputs))
            .validate(true)?;
    }
    // Paths bound to check the source files are bound for the DAG too.
    let mut paths = None;
    if let Some(root) = &prepared.root {
        let (verified, bound) = validate_bound_source_files(&prepared.bound, dag, root)?;
        eprintln!("note: {verified}");
        paths = Some(bound);
    }
    eprintln!("note: {}", job_count(&prepared.pipeline, dag));
    let bind = |paths: Option<BoundPaths>| match paths {
        Some(paths) => bind_dag_with(&prepared.bound, dag, paths),
        None => bind_dag(&prepared.bound, dag),
    };
    if args.has(Flag::Output) || args.has(Flag::Json) {
        let mut bound = bind(paths)?;
        bound.root = prepared.root.as_deref().map(|root| {
            std::path::absolute(root)
                .unwrap_or_else(|_| root.to_path_buf())
                .to_string_lossy()
                .into_owned()
        });
        return write_spitdag(args, &bound);
    }
    if args.has(Flag::Paths) {
        print!("{}", render_bound_dag(&bind(paths)?, true));
    } else {
        print!("{}", render_dag(dag));
    }
    Ok(())
}

/// Step 3: what can be made, what cannot, and why.
fn artifacts(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(args)?;
    let mut report = prepared.report;
    report.coverage = prepared.inputs.gaps;
    if let Some(root) = &prepared.root {
        validate_source_files(&prepared.bound, &report.dag, root)?;
    }
    print!("{}", render_artifacts(&report));
    Ok(())
}

/// Print the `.spitdag`, or write it to the `-o` file, a piece at a time.
fn write_spitdag(args: &CliArgs, bound: &BoundDag) -> Result<(), Box<dyn Error>> {
    match args.value(Flag::Output) {
        Some(file) => {
            let written = fs::File::create(&file).and_then(|mut out| bound.write_json(&mut out));
            written.map_err(|reason| format!("cannot write `{file}`: {reason}"))?;
            eprintln!("note: wrote the .spitdag to `{file}`");
        }
        None => bound.write_json(&mut io::stdout().lock())?,
    }
    Ok(())
}

/// Print `text`, or write it to the `-o` file.
fn write_output(args: &CliArgs, text: &str, what: &str) -> Result<(), Box<dyn Error>> {
    match args.value(Flag::Output) {
        Some(file) => {
            fs::write(&file, text).map_err(|reason| format!("cannot write `{file}`: {reason}"))?;
            eprintln!("note: wrote {what} to `{file}`");
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// Sources whose records give their files, which then need no path rule.
fn located(inputs: &ResolvedInputs) -> impl Iterator<Item = &str> {
    inputs
        .inventory
        .artifacts
        .iter()
        .filter(|record| record.path.is_some())
        .map(|record| record.product.as_str())
}

fn is_recipe(file: &str) -> bool {
    Path::new(file)
        .extension()
        .is_some_and(|extension| extension == "spitin")
}

/// How many jobs resolved, per stage when the pipeline has stages.
fn job_count(pipeline: &Pipeline, dag: &ResolvedDag) -> String {
    let total = dag.jobs.len();
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
    Ok(text)
}

/// Read a file, naming it if it cannot be read.
fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|reason| format!("cannot read `{path}`: {reason}"))
}

/// Print every diagnostic, failing if any is an error.
fn report(
    diagnostics: &[Diagnostic],
    text: &str,
    inventory_text: Option<&str>,
) -> Result<(), Reported> {
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.display_in(text, inventory_text));
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(Reported);
    }
    Ok(())
}

/// Print what `diagnosis` found, and pass on what it checked; `warnings`
/// finds its warnings.
fn passed<T>(
    diagnosis: Diagnosis<T>,
    warnings: fn(&T) -> &[Diagnostic],
    text: &str,
    inventory_text: Option<&str>,
) -> Result<T, Reported> {
    match diagnosis {
        Ok(checked) => {
            report(warnings(&checked), text, inventory_text)?;
            Ok(checked)
        }
        Err(all) => {
            report(&all, text, inventory_text)?;
            Err(Reported)
        }
    }
}
