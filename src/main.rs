use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::ExitCode;

use spit::{
    diagnose_at, inspect_paths, parse_document_at, parse_source_inventory, render_bash,
    render_bound_dag, render_dag, resolve, validate_concrete_paths, validate_source_files,
};

const USAGE: &str =
    "usage: spit <check|dag|bound-dag|paths|bash|diagnose> <pipeline.spit> [--sources <inventory.spit|->] [--root <directory>] [--strict-paths]";

struct CliArgs {
    command: String,
    pipeline: String,
    sources: Option<String>,
    strict_paths: bool,
    root: Option<String>,
}

fn parse_args() -> Result<CliArgs, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(USAGE)?;
    if !matches!(
        command.as_str(),
        "check" | "dag" | "bound-dag" | "paths" | "bash" | "diagnose"
    ) {
        return Err(USAGE.into());
    }
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
    if args.command == "diagnose" {
        return run_diagnose(&args);
    }
    let pipeline_text = fs::read_to_string(&args.pipeline)?;
    let inventory_text = match args.sources.as_deref() {
        Some("-") => {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text)?;
            Some(text)
        }
        Some(sources) => Some(fs::read_to_string(sources)?),
        None => None,
    };
    // Report every error and warning before doing any work.
    let path = Path::new(&args.pipeline);
    let diagnostics = diagnose_at(&pipeline_text, inventory_text.as_deref(), path);
    for diagnostic in &diagnostics {
        eprintln!("{diagnostic}");
    }
    if diagnostics.iter().any(|diagnostic| diagnostic.is_error()) {
        return Err(Reported.into());
    }
    let (pipeline, embedded_inventory) = parse_document_at(&pipeline_text, path)?;
    let coverage = inspect_paths(&pipeline)?;
    let inventory = match &inventory_text {
        Some(text) => Some(parse_source_inventory(text)?),
        None => embedded_inventory,
    };
    let Some(inventory) = inventory else {
        if args.command == "check" && args.root.is_none() {
            if args.strict_paths {
                coverage.validate(true)?;
            }
            println!("Pipeline valid.\n\nNo source inventory; jobs not resolved.");
            return Ok(());
        }
        return Err("no inline source inventory; supply --sources <inventory.spit|->".into());
    };
    let dag = resolve(&pipeline, &inventory)?;
    if args.strict_paths || args.command == "paths" {
        if args.command == "paths" {
            print!("{coverage}");
        }
        coverage.validate(args.strict_paths)?;
        validate_concrete_paths(&pipeline, &dag)?;
    }
    let checked_files = args
        .root
        .as_ref()
        .map(|root| validate_source_files(&pipeline, &dag, std::path::Path::new(root)))
        .transpose()?;
    match args.command.as_str() {
        "check" => {
            println!("Pipeline valid.\n\n{} jobs resolved.", dag.jobs.len());
            if let Some(count) = checked_files {
                println!("{count} source files verified.");
            }
        }
        "dag" => print!("{}", render_dag(&dag)),
        "bound-dag" => print!("{}", render_bound_dag(&pipeline, &dag)?),
        "paths" => (),
        "bash" => print!("{}", render_bash(&pipeline, &dag)?),
        _ => unreachable!(),
    }
    Ok(())
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
    print!("{{\"diagnostics\":[");
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index != 0 {
            print!(",");
        }
        print!(
            "{{\"severity\":\"{}\",\"source\":\"{}\",\"line\":{},\"message\":\"{}\"}}",
            diagnostic.severity.as_str(),
            diagnostic.source,
            diagnostic
                .line
                .map_or_else(|| "null".to_owned(), |line| line.to_string()),
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
            c if c < ' ' => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped
}
