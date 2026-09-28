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

const USAGE: &str =
    "usage: spit <check|dag|bash|artifacts|discover> <pipeline.spit> [--sources <inventory.spit|->] [--root <directory>] [--stage <name>] [--paths] [--strict-paths] [--json] [--stdin]";

#[derive(Clone, Copy, PartialEq)]
enum Command {
    Check,
    Dag,
    Bash,
    Artifacts,
    Discover,
}

impl Command {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "check" => Self::Check,
            "dag" => Self::Dag,
            "bash" => Self::Bash,
            "artifacts" => Self::Artifacts,
            "discover" => Self::Discover,
            _ => return None,
        })
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

fn parse_args() -> Result<CliArgs, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let command = args
        .next()
        .as_deref()
        .and_then(Command::parse)
        .ok_or(USAGE)?;
    let pipeline = args.next().ok_or(USAGE)?;
    if pipeline.starts_with("--") {
        return Err(USAGE.into());
    }
    let mut sources = None;
    let mut paths = false;
    let mut strict_paths = false;
    let mut root = None;
    let mut stage = None;
    let mut json = false;
    let mut stdin = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--sources" if sources.is_none() => sources = Some(args.next().ok_or(USAGE)?),
            "--paths" if !paths => paths = true,
            "--strict-paths" if !strict_paths => strict_paths = true,
            "--root" if root.is_none() => root = Some(args.next().ok_or(USAGE)?),
            "--stage" if stage.is_none() => stage = Some(args.next().ok_or(USAGE)?),
            "--json" if !json => json = true,
            "--stdin" if !stdin => stdin = true,
            _ => return Err(USAGE.into()),
        }
    }
    Ok(CliArgs {
        command,
        pipeline,
        sources,
        paths,
        strict_paths,
        root,
        stage,
        json,
        stdin,
    })
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
    if args.json
        && (args.command != Command::Check
            || args.paths
            || args.strict_paths
            || args.root.is_some())
    {
        return Err("--json applies to check without --paths, --strict-paths, or --root".into());
    }
    if args.stdin && args.sources.as_deref() == Some("-") {
        return Err("--stdin reads the pipeline, so --sources needs a file".into());
    }
    let pipeline_text = if args.stdin {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text)?;
        text
    } else {
        fs::read_to_string(&args.pipeline)?
    };
    let path = Path::new(&args.pipeline);
    if args.command == Command::Discover && (args.root.is_none() || args.sources.is_some()) {
        return Err("discover reads files under --root <directory> and takes no --sources".into());
    }
    if args.command == Command::Artifacts && args.strict_paths {
        return Err("artifacts does not support --strict-paths".into());
    }
    if args.paths && !matches!(args.command, Command::Check | Command::Dag) {
        return Err("--paths applies to check and dag".into());
    }
    if args.stage.is_some()
        && (!matches!(args.command, Command::Check | Command::Dag | Command::Bash) || args.json)
    {
        return Err("--stage applies to check, dag, and bash, without --json".into());
    }
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
