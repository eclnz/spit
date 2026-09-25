use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

use spit::{
    inspect_paths, parse_document, parse_source_inventory, render_bash, render_dag, resolve,
    validate_concrete_paths,
};

const USAGE: &str =
    "usage: spit <check|dag|paths|bash> <pipeline.spit> [--sources <inventory.spit|->] [--strict-paths]";

struct CliArgs {
    command: String,
    pipeline: String,
    sources: Option<String>,
    strict_paths: bool,
}

fn parse_args() -> Result<CliArgs, Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(USAGE)?;
    if !matches!(command.as_str(), "check" | "dag" | "paths" | "bash") {
        return Err(USAGE.into());
    }
    let pipeline = args.next().ok_or(USAGE)?;
    if pipeline.starts_with("--") {
        return Err(USAGE.into());
    }
    let mut sources = None;
    let mut strict_paths = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--sources" if sources.is_none() => sources = Some(args.next().ok_or(USAGE)?),
            "--strict-paths" if !strict_paths => strict_paths = true,
            _ => return Err(USAGE.into()),
        }
    }
    Ok(CliArgs {
        command,
        pipeline,
        sources,
        strict_paths,
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
    match args.command.as_str() {
        "check" => println!("Pipeline valid.\n\n{} jobs resolved.", dag.jobs.len()),
        "dag" => print!("{}", render_dag(&dag)),
        "paths" => (),
        "bash" => print!("{}", render_bash(&pipeline, &dag)?),
        _ => unreachable!(),
    }
    Ok(())
}
