//! The `spit` command line. Each command is one step, and the files given
//! say what it works on:
//!
//! 1. `check` compiles a pipeline, or checks a recipe against its pipeline;
//! 2. `inputs` settles a dataset from a recipe, writing a `.spitout`;
//! 3. `dag` and `artifacts` resolve a pipeline's jobs over a `.spitout`.
//!
//! A command given files from an earlier step runs the steps between in
//! memory. Nothing is loaded that the command line does not name.

use std::env;
use std::error::Error;
use std::fmt::Write;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use spit::{
    bind_dag, diagnose_artifacts_at, diagnose_at, diagnose_at_with_inputs, diagnose_recipe,
    inspect_paths, parse_input_spec_at, parse_pipeline_at, parse_source_inventory,
    render_artifacts, render_bound_dag, render_dag, render_source_inventory, resolve,
    resolve_artifacts_excluding, stage_within, validate_pipeline, validate_source_files,
    Diagnostic, InputSource, InputSpec, PathTemplate, Pipeline, ResolvedDag, ResolvedInputs,
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

impl Command {
    fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Inputs => "inputs",
            Self::Dag => "dag",
            Self::Artifacts => "artifacts",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        COMMANDS.into_iter().find(|command| command.name() == name)
    }

    /// The files it takes, as the usage line shows them.
    fn files(self) -> &'static str {
        match self {
            Self::Check => "<pipeline.spit | recipe.spitin>",
            Self::Inputs => "<recipe.spitin>",
            Self::Dag | Self::Artifacts => "<pipeline.spit> <inputs.spitout | recipe.spitin | ->",
        }
    }

    /// The fewest and most files it takes.
    fn arity(self) -> (usize, usize) {
        match self {
            Self::Check | Self::Inputs => (1, 1),
            Self::Dag | Self::Artifacts => (2, 2),
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::Check => "step 1: compile a pipeline, or check a recipe against its pipeline; reads no data",
            Self::Inputs => "step 2: find a dataset's sources with a recipe, apply `skip` and `require`, and write a .spitout",
            Self::Dag => "step 3: resolve a pipeline's jobs over a dataset's inputs; -o writes the .spitdag",
            Self::Artifacts => "step 3: report what can and cannot be made from a dataset's inputs, and why",
        }
    }

    fn example(self) -> &'static str {
        match self {
            Self::Check => "spit check analysis.spit\n  spit check dataset.spitin",
            Self::Inputs => "spit inputs dataset.spitin -o dataset.spitout",
            Self::Dag => "spit dag analysis.spit dataset.spitout -o analysis.spitdag\n  spit dag analysis.spit dataset.spitout --paths",
            Self::Artifacts => "spit artifacts analysis.spit dataset.spitout",
        }
    }

    /// Given a recipe in place of a `.spitout`, or earlier files in place of
    /// a `.spitdag`, the command runs the steps between in memory.
    fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Dag | Self::Artifacts => Some(
                "Given a .spitin in place of the .spitout, it runs `spit inputs` in memory first.",
            ),
            Self::Check | Self::Inputs => None,
        }
    }

    /// The flags this command accepts.
    fn flags(self) -> &'static [Flag] {
        use Flag::*;
        match self {
            Self::Check => &[PathRules, StrictPaths, Json, Stdin],
            Self::Inputs => &[Root, Output],
            Self::Dag => &[Root, StrictPaths, Paths, Json, Output],
            Self::Artifacts => &[Root],
        }
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
}

const FLAGS: [Flag; 7] = [
    Flag::Root,
    Flag::Output,
    Flag::Paths,
    Flag::PathRules,
    Flag::StrictPaths,
    Flag::Json,
    Flag::Stdin,
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
    files: Vec<String>,
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

fn overview() -> String {
    let mut text = String::from(
        "spit: compile a pipeline, settle a dataset's inputs, resolve jobs, and write a script\n\nusage: spit <command> <files> [options]\n\ncommands:\n",
    );
    for command in COMMANDS {
        writeln!(text, "  {:<10} {}", command.name(), command.summary()).unwrap();
    }
    text.push_str(
        "\nfiles:\n  .spit      a pipeline: sources, operations, steps, commands, path rules\n  .spitin    a recipe for a dataset's inputs, naming its pipeline\n  .spitout   a dataset's settled inputs, each source with its file\n  .spitdag   the resolved jobs, each with its files and command\n\nRun `spit help <command>` for its options.\n",
    );
    text
}

fn command_help(command: Command) -> String {
    let mut text = format!(
        "spit {}: {}\n\nusage: spit {} {} [options]\n",
        command.name(),
        command.summary(),
        command.name(),
        command.files()
    );
    if let Some(shortcut) = command.shortcut() {
        writeln!(text, "\n{shortcut}").unwrap();
    }
    if !command.flags().is_empty() {
        text.push_str("\noptions:\n");
        for flag in command.flags() {
            let name = match flag.value() {
                Some(value) => format!("{} {value}", flag.name()),
                None => flag.name().to_owned(),
            };
            writeln!(text, "  {name:<20} {}", flag.help(command)).unwrap();
        }
    }
    writeln!(text, "\nexample:\n  {}", command.example()).unwrap();
    text
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
        let flag = Flag::parse(&argument)
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
        if flags.has(flag) {
            return Err(misuse(
                format_args!("{} is given more than once", flag.name()),
                Some(command),
            ));
        }
        let value = match flag.value() {
            Some(value) => Some(args.next().ok_or_else(|| {
                misuse(
                    format_args!("{} needs a value: {value}", flag.name()),
                    Some(command),
                )
            })?),
            None => None,
        };
        flags.0.push((flag, value));
    }
    for (first, second) in CONFLICTS {
        if flags.has(first) && flags.has(second) {
            return Err(misuse(
                format_args!("{} cannot be used with {}", first.name(), second.name()),
                Some(command),
            ));
        }
    }
    let (fewest, most) = command.arity();
    if files.len() < fewest {
        return Err(misuse(
            format_args!("{} needs {}", command.name(), command.files()),
            Some(command),
        ));
    }
    if files.len() > most {
        return Err(misuse(
            format_args!("unexpected file `{}`", files[most]),
            Some(command),
        ));
    }
    Ok(Request::Run(CliArgs {
        command,
        files,
        flags,
    }))
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
            print!("{}", command.map_or_else(overview, command_help));
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

/// Step 1: compile a pipeline, or check a recipe against the pipeline it
/// names. Reads no data.
fn check(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let file = &args.files[0];
    let path = Path::new(file);
    let text = if args.has(Flag::Stdin) {
        read_stdin()?
    } else {
        read_file(file)?
    };
    if is_recipe(file) {
        if args.has(Flag::PathRules) || args.has(Flag::StrictPaths) {
            return Err("--path-rules and --strict-paths check a pipeline, not a recipe".into());
        }
        let diagnostics = diagnose_recipe(&text, path);
        if args.has(Flag::Json) {
            print_json(&diagnostics, &text, None);
            return Ok(());
        }
        report(&diagnostics, &text, None)?;
        println!("Recipe valid.");
        return Ok(());
    }
    let diagnostics = diagnose_at(&text, None, path);
    if args.has(Flag::Json) {
        print_json(&diagnostics, &text, None);
        return Ok(());
    }
    report(&diagnostics, &text, None)?;
    let coverage = inspect_paths(&parse_pipeline_at(&text, path)?)?;
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
    let settled = run_inputs(&args.files[0], None, args.value(Flag::Root).as_deref())?;
    settled.inputs.require_complete()?;
    let text = render_source_inventory(&settled.inputs.inventory, &settled.pipeline);
    write_output(args, &text, "the .spitout")
}

/// A settled dataset, with the pipeline it was settled for.
struct Settled {
    pipeline: Pipeline,
    recipe: InputSpec,
    inputs: ResolvedInputs,
}

/// Run step 2 for the recipe `file`. `pipeline` is the pipeline the caller
/// was given, which the recipe's `pipeline` line must match.
fn run_inputs(
    file: &str,
    pipeline: Option<&Path>,
    root: Option<&str>,
) -> Result<Settled, Box<dyn Error>> {
    if !is_recipe(file) {
        return Err(format!("spit inputs reads a .spitin recipe, not `{file}`").into());
    }
    let folder = Path::new(file)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_owned();
    let root = root.map_or(folder, PathBuf::from);
    let recipe = parse_input_spec_at(&read_file(file)?, Path::new(file))
        .map_err(|error| format!("{file}: {error}"))?;
    let pipeline_file = match (recipe.pipeline.clone(), pipeline) {
        (Some(named), Some(given)) if !same_file(&named, given) => {
            return Err(format!(
                "{file} is a recipe for `{}`, not `{}`",
                named.display(),
                given.display()
            )
            .into())
        }
        (_, Some(given)) => given.to_owned(),
        (Some(named), None) => named,
        (None, None) => {
            return Err(format!(
                "{file} does not name its pipeline; add a line such as `pipeline analysis.spit`"
            )
            .into())
        }
    };
    let pipeline_text = read_file(&pipeline_file.display().to_string())?;
    report(
        &diagnose_at(&pipeline_text, None, &pipeline_file),
        &pipeline_text,
        None,
    )?;
    let pipeline = parse_pipeline_at(&pipeline_text, &pipeline_file)?;
    validate_pipeline(&pipeline)?;
    let source = match &recipe.inventory {
        Some(records) => InputSource::Inventory(records.clone()),
        None => InputSource::Discover(&root),
    };
    let resolved = recipe.resolve(&pipeline, source)?;
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
    Ok(Settled {
        pipeline,
        recipe,
        inputs: resolved,
    })
}

/// A pipeline ready for step 3: its settled inputs, and the pipeline used
/// to bind paths.
struct Prepared {
    pipeline: Pipeline,
    bound: Pipeline,
    inputs: ResolvedInputs,
    /// Where source files are, when known.
    root: Option<PathBuf>,
}

/// Read the pipeline and its inputs for step 3, running step 2 in memory
/// for a recipe, and report every diagnostic first.
fn prepare(args: &CliArgs, pipeline_file: &str, inputs: &str) -> Result<Prepared, Box<dyn Error>> {
    let path = Path::new(pipeline_file);
    let pipeline_text = read_file(pipeline_file)?;
    let lenient = args.command == Command::Artifacts;
    let mut root = args.value(Flag::Root).map(PathBuf::from);
    let (records_text, recipe) = if inputs == "-" {
        (read_stdin()?, None)
    } else if is_recipe(inputs) {
        let given_root = root.as_ref().and_then(|root| root.to_str());
        let settled = run_inputs(inputs, Some(path), given_root)?;
        eprintln!("note: ran `spit inputs {inputs}` in memory");
        root = root.or_else(|| settled.inputs.root.clone());
        let text = render_source_inventory(&settled.inputs.inventory, &settled.pipeline);
        (text, Some(settled.recipe))
    } else {
        (read_file(inputs)?, None)
    };
    let diagnostics = match &recipe {
        Some(recipe) => {
            diagnose_at_with_inputs(&pipeline_text, Some(&records_text), path, recipe, lenient)
        }
        None if lenient => diagnose_artifacts_at(&pipeline_text, Some(&records_text), path),
        None => diagnose_at(&pipeline_text, Some(&records_text), path),
    };
    report(&diagnostics, &pipeline_text, Some(&records_text))?;
    let pipeline = parse_pipeline_at(&pipeline_text, path)?;
    let records = parse_source_inventory(&records_text)?;
    let settled = recipe
        .unwrap_or_default()
        .resolve(&pipeline, InputSource::Inventory(records))?;
    // Records give every source its file; outputs with no rule take the
    // built-in layout.
    let mut bound = pipeline.clone();
    bound
        .path_template
        .get_or_insert_with(PathTemplate::default_output);
    Ok(Prepared {
        pipeline,
        bound,
        inputs: settled,
        root,
    })
}

/// Step 3: resolve the jobs and print them, or write the `.spitdag`.
fn dag(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(args, &args.files[0], &args.files[1])?;
    prepared.inputs.require_complete()?;
    let dag = resolve(&prepared.pipeline, &prepared.inputs.dag_inventory())?;
    if args.has(Flag::StrictPaths) {
        inspect_paths(&prepared.bound)?
            .with_inventory_paths(located(&prepared.inputs))
            .validate(true)?;
    }
    if let Some(root) = &prepared.root {
        let verified = validate_source_files(&prepared.bound, &dag, root)?;
        eprintln!("note: {verified}");
    }
    eprintln!("note: {}", job_count(&prepared.pipeline, &dag));
    if args.has(Flag::Output) {
        let bound = bind_dag(&prepared.bound, &dag)?;
        return write_output(args, &bound.to_json(), "the .spitdag");
    }
    if args.has(Flag::Json) {
        print!("{}", bind_dag(&prepared.bound, &dag)?.to_json());
    } else if args.has(Flag::Paths) {
        print!(
            "{}",
            render_bound_dag(&bind_dag(&prepared.bound, &dag)?, true)
        );
    } else {
        print!("{}", render_dag(&dag));
    }
    Ok(())
}

/// Step 3: what can be made, what cannot, and why.
fn artifacts(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(args, &args.files[0], &args.files[1])?;
    let mut report = resolve_artifacts_excluding(
        &prepared.pipeline,
        &prepared.inputs.dag_inventory(),
        &prepared.inputs.unavailable(),
    )?;
    report.coverage = prepared.inputs.gaps;
    if let Some(root) = &prepared.root {
        validate_source_files(&prepared.bound, &report.dag, root)?;
    }
    print!("{}", render_artifacts(&report));
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

fn same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => first == second,
    }
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
    Ok(strip_bom(text))
}

/// Drop a UTF-8 byte order mark, which some Windows editors write.
fn strip_bom(text: String) -> String {
    match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    }
}

/// Read a file, naming it if it cannot be read.
fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path)
        .map(strip_bom)
        .map_err(|reason| format!("cannot read `{path}`: {reason}"))
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
