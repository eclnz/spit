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
    render_dag, render_source_inventory, resolve, resolve_artifacts, validate_concrete_paths,
    validate_source_files,
};

const USAGE: &str =
    "usage: spit <check|dag|bound-dag|paths|bash|artifacts|discover|diagnose> <pipeline.spit> [--sources <inventory.spit|->] [--root <directory>] [--strict-paths]";

#[derive(Clone, Copy, PartialEq)]
enum Command {
    Check,
    Dag,
    BoundDag,
    Paths,
    Bash,
    Artifacts,
    Discover,
    Diagnose,
}

impl Command {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "check" => Self::Check,
            "dag" => Self::Dag,
            "bound-dag" => Self::BoundDag,
            "paths" => Self::Paths,
            "bash" => Self::Bash,
            "artifacts" => Self::Artifacts,
            "discover" => Self::Discover,
            "diagnose" => Self::Diagnose,
            _ => return None,
        })
    }
}

struct CliArgs {
    command: Command,
    pipeline: String,
    sources: Option<String>,
    strict_paths: bool,
    root: Option<String>,
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
    let mut strict_paths = false;
    let mut root = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--sources" if sources.is_none() => sources = Some(args.next().ok_or(USAGE)?),
            "--strict-paths" if !strict_paths => strict_paths = true,
            "--root" if root.is_none() => root = Some(args.next().ok_or(USAGE)?),
            _ => return Err(USAGE.into()),
        }
    }
    Ok(CliArgs {
        command,
        pipeline,
        sources,
        strict_paths,
        root,
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
    if args.command == Command::Diagnose {
        return run_diagnose(&args);
    }
    let pipeline_text = fs::read_to_string(&args.pipeline)?;
    let path = Path::new(&args.pipeline);
    if args.command == Command::Discover && (args.root.is_none() || args.sources.is_some()) {
        return Err("discover reads files under --root <directory> and takes no --sources".into());
    }
    if args.command == Command::Artifacts && args.strict_paths {
        return Err("artifacts does not support --strict-paths".into());
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
    let coverage = inspect_paths(&pipeline)?;
    let Some(inventory) = inventory else {
        if args.command == Command::Check && args.root.is_none() {
            if args.strict_paths {
                coverage.validate(true)?;
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
    if args.strict_paths || args.command == Command::Paths {
        if args.command == Command::Paths {
            print!("{coverage}");
        }
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
            println!("Pipeline valid.\n\n{} jobs resolved.", dag.jobs.len());
            if let Some(count) = checked_files {
                println!("{count} source files verified.");
            }
        }
        Command::Dag => print!("{}", render_dag(&dag)),
        Command::BoundDag => print!("{}", render_bound_dag(&pipeline, &dag)?),
        Command::Paths => {}
        Command::Bash => print!("{}", render_bash(&pipeline, &dag)?),
        Command::Artifacts | Command::Discover | Command::Diagnose => {
            unreachable!("handled before resolution")
        }
    }
    Ok(())
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

fn run_diagnose(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    if args.strict_paths || args.root.is_some() {
        return Err("diagnose does not support --strict-paths or --root".into());
    }
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    let source_text = match &args.sources {
        Some(source) if source == "-" => {
            return Err("diagnose reads the pipeline from stdin; --sources needs a file".into());
        }
        Some(source) => Some(fs::read_to_string(source)?),
        None => None,
    };
    let diagnostics = diagnose_at(&text, source_text.as_deref(), Path::new(&args.pipeline));
    let number = |value: Option<usize>| value.map_or_else(|| "null".to_owned(), |n| n.to_string());
    print!("{{\"diagnostics\":[");
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index != 0 {
            print!(",");
        }
        // Columns are 1-based, in UTF-16 code units as editors count them;
        // `end_column` is one past the last character.
        let columns = diagnostic.utf16_columns(&text, source_text.as_deref());
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
    Ok(())
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
