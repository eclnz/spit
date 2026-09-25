use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

use spit::{
    inspect_paths, parse_document, parse_source_inventory, render_bash, render_bound_dag,
    render_dag, resolve, validate_concrete_paths, validate_source_files,
};

const USAGE: &str =
    "usage: spit <check|dag|bound-dag|paths|bash> <pipeline.spit> [--sources <inventory.spit|->] [--root <directory>] [--strict-paths]";

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
        "check" | "dag" | "bound-dag" | "paths" | "bash"
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
    let (pipeline, embedded_inventory) = parse_document(&fs::read_to_string(&args.pipeline)?)?;
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
    if args.strict_paths || args.command == "paths" {
        let coverage = inspect_paths(&pipeline)?;
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
