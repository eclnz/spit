use std::env;
use std::error::Error;
use std::fmt::Write;
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

#[derive(Clone, Copy, PartialEq)]
enum Command {
    Check,
    Dag,
    BoundDag,
    Paths,
    Bash,
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    if args.command == Command::Diagnose {
        return run_diagnose(&args);
    }
    let (pipeline, embedded_inventory) = parse_document_at(
        &fs::read_to_string(&args.pipeline)?,
        Path::new(&args.pipeline),
    )?;
    let inventory = if let Some(sources) = &args.sources {
        let inventory_text = if sources == "-" {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text)?;
            text
        } else {
            fs::read_to_string(sources)?
        };
        parse_source_inventory(&inventory_text)?
    } else {
        embedded_inventory
            .ok_or("no inline source inventory; supply --sources <inventory.spit|->")?
    };
    let dag = resolve(&pipeline, &inventory)?;
    if args.strict_paths || args.command == Command::Paths {
        let coverage = inspect_paths(&pipeline)?;
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
        Command::Diagnose => unreachable!("handled before resolution"),
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
            "{{\"source\":\"{}\",\"line\":{},\"message\":\"{}\"}}",
            diagnostic.source.as_str(),
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
            c if c < ' ' => write!(escaped, "\\u{:04x}", u32::from(c)).unwrap(),
            c => escaped.push(c),
        }
    }
    escaped
}
