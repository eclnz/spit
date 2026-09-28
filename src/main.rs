use std::env;
use std::error::Error;
use std::fmt::Write;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::ExitCode;

use spit::{
    diagnose_artifacts_at, diagnose_at, discover_sources, inspect_paths, parse_document_at,
    parse_pipeline_at, parse_source_inventory, render_artifacts, render_bash, render_bound_dag,
    render_dag, render_source_inventory, resolve, resolve_artifacts, stage_within,
    validate_concrete_paths, validate_source_files, Diagnostic, Pipeline, ResolvedDag,
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
            Self::Check => &[Sources, Root, Stage, Paths, StrictPaths, Json, Stdin],
            Self::Dag => &[Sources, Root, Stage, Paths, StrictPaths, Stdin],
            Self::Bash => &[Sources, Root, Stage, StrictPaths, Stdin],
            Self::Artifacts => &[Sources, Root, Stdin],
            Self::Discover => &[Root, Stdin],
        }
    }

    /// The flags this command cannot run without.
    fn required(self) -> &'static [Flag] {
        match self {
            Self::Discover => &[Flag::Root],
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Flag {
    Sources,
    Root,
    Stage,
    Paths,
    StrictPaths,
    Json,
    Stdin,
}

const FLAGS: [Flag; 7] = [
    Flag::Sources,
    Flag::Root,
    Flag::Stage,
    Flag::Paths,
    Flag::StrictPaths,
    Flag::Json,
    Flag::Stdin,
];

/// Pairs of flags that cannot be used together, whatever the command.
const CONFLICTS: [(Flag, Flag); 4] = [
    (Flag::Json, Flag::Paths),
    (Flag::Json, Flag::StrictPaths),
    (Flag::Json, Flag::Root),
    (Flag::Json, Flag::Stage),
];

impl Flag {
    fn name(self) -> &'static str {
        match self {
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
            Self::Sources => Some("<inventory.spit|->"),
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

fn parse_args() -> Result<CliArgs, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let command = args
        .next()
        .as_deref()
        .and_then(Command::parse)
        .ok_or_else(usage)?;
    let pipeline = args.next().ok_or_else(usage)?;
    if pipeline.starts_with("--") {
        return Err(usage().into());
    }
    let mut flags = Flags::default();
    while let Some(name) = args.next() {
        let flag = Flag::parse(&name)
            .filter(|flag| !flags.has(*flag))
            .ok_or_else(usage)?;
        let value = match flag.value() {
            Some(_) => Some(args.next().ok_or_else(usage)?),
            None => None,
        };
        flags.0.push((flag, value));
    }
    check_flags(command, &flags)?;
    Ok(CliArgs {
        command,
        pipeline,
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
        if flags.has(first) && flags.has(second) {
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
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if !error.is::<Reported>() {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let pipeline_text = if args.stdin {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text)?;
        text
    } else {
        fs::read_to_string(&args.pipeline)?
    };
    let path = Path::new(&args.pipeline);
    let inventory_text = match (args.sources.as_deref(), &args.root) {
        (Some("-"), _) => {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text)?;
            Some(text)
        }
        (Some(sources), _) => Some(fs::read_to_string(sources)?),
        // With a root and no inventory, find the sources by their path rules.
        (None, Some(root))
            if args.command == Command::Discover || !has_inline_inventory(&pipeline_text, path) =>
        {
            discover(&pipeline_text, path, Path::new(root))?
        }
        (None, _) => None,
    };
    // Report every error and warning before doing any work.
    let diagnose = if args.command == Command::Artifacts {
        diagnose_artifacts_at
    } else {
        diagnose_at
    };
    let diagnostics = diagnose(&pipeline_text, inventory_text.as_deref(), path);
    if args.json {
        print_json(&diagnostics, &pipeline_text, inventory_text.as_deref());
        return Ok(());
    }
    for diagnostic in &diagnostics {
        eprintln!(
            "{}",
            diagnostic.display_in(&pipeline_text, inventory_text.as_deref())
        );
    }
    if diagnostics.iter().any(|diagnostic| diagnostic.is_error()) {
        return Err(Reported.into());
    }
    if args.command == Command::Discover {
        print!("{}", inventory_text.unwrap_or_default());
        return Ok(());
    }
    // A separate inventory replaces an inline one, which is then not read.
    let (pipeline, inventory) = match &inventory_text {
        Some(text) => (
            parse_pipeline_at(&pipeline_text, path)?,
            Some(parse_source_inventory(text)?),
        ),
        None => parse_document_at(&pipeline_text, path)?,
    };
    if let Some(stage) = &args.stage {
        if !pipeline
            .stages
            .iter()
            .any(|declared| &declared.name == stage)
        {
            let names: Vec<_> = pipeline
                .stages
                .iter()
                .map(|stage| format!("`{}`", stage.name))
                .collect();
            return Err(if names.is_empty() {
                format!("unknown stage `{stage}`; this pipeline declares no stages").into()
            } else {
                format!("unknown stage `{stage}`; stages: {}", names.join(", ")).into()
            });
        }
    }
    let coverage = inspect_paths(&pipeline)?;
    if args.command == Command::Check && args.paths {
        println!("{coverage}");
    }
    let Some(inventory) = inventory else {
        if args.command == Command::Check && args.root.is_none() {
            if args.strict_paths || args.paths {
                coverage.validate(args.strict_paths)?;
            }
            println!("Pipeline valid.\n\nNo source inventory; jobs not resolved.");
            return Ok(());
        }
        return Err("no inline source inventory; supply --sources <inventory.spit|->".into());
    };
    if args.command == Command::Artifacts {
        let report = resolve_artifacts(&pipeline, &inventory)?;
        if let Some(root) = &args.root {
            validate_source_files(&pipeline, &report.dag, Path::new(root))?;
        }
        print!("{}", render_artifacts(&report));
        return Ok(());
    }
    let dag = resolve(&pipeline, &inventory)?;
    // A single stage runs on the files earlier stages already wrote.
    let dag = match &args.stage {
        Some(stage) => dag.only_stage(stage),
        None => dag,
    };
    if args.strict_paths || args.paths {
        coverage.validate(args.strict_paths)?;
        validate_concrete_paths(&pipeline, &dag)?;
    }
    let checked_files = args
        .root
        .as_ref()
        .map(|root| validate_source_files(&pipeline, &dag, Path::new(root)))
        .transpose()?;
    match args.command {
        Command::Check => {
            println!(
                "Pipeline valid.\n\n{}",
                job_count(&pipeline, &dag, args.stage.as_deref())
            );
            if let Some(verified) = checked_files {
                println!("{verified}");
            }
        }
        Command::Dag if args.paths => print!("{}", render_bound_dag(&pipeline, &dag)?),
        Command::Dag => print!("{}", render_dag(&dag)),
        Command::Bash => print!("{}", render_bash(&pipeline, &dag)?),
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

fn has_inline_inventory(text: &str, path: &Path) -> bool {
    parse_document_at(text, path).is_ok_and(|(_, inventory)| inventory.is_some())
}

/// The inventory text for the source files under `root`, or `None` when the
/// pipeline does not parse; diagnostics then report why.
fn discover(text: &str, path: &Path, root: &Path) -> Result<Option<String>, Box<dyn Error>> {
    let Ok(pipeline) = parse_pipeline_at(text, path) else {
        return Ok(None);
    };
    let inventory = discover_sources(&pipeline, root)?;
    eprintln!(
        "note: discovered {} source artifacts under `{}`",
        inventory.artifacts.len(),
        root.display()
    );
    Ok(Some(render_source_inventory(&inventory, &pipeline)))
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
