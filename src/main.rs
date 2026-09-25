use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

use spit::{parse_document, parse_source_inventory, render_dag, resolve};

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
    let args: Vec<_> = env::args().skip(1).collect();
    if !matches!(args.first().map(String::as_str), Some("check" | "dag"))
        || !matches!(args.len(), 2 | 4)
        || (args.len() == 4 && args[2] != "--sources")
    {
        return Err(
            "usage: spit <check|dag> <pipeline.spit> [--sources <inventory.spit|->]".into(),
        );
    }
    let (pipeline, embedded_inventory) = parse_document(&fs::read_to_string(&args[1])?)?;
    let inventory = if args.len() == 4 {
        let inventory_text = if args[3] == "-" {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text)?;
            text
        } else {
            fs::read_to_string(&args[3])?
        };
        parse_source_inventory(&inventory_text)?
    } else {
        embedded_inventory
            .ok_or("no inline source inventory; supply --sources <inventory.spit|->")?
    };
    let dag = resolve(&pipeline, &inventory)?;
    match args[0].as_str() {
        "check" => println!("Pipeline valid.\n\n{} jobs resolved.", dag.jobs.len()),
        "dag" => print!("{}", render_dag(&dag)),
        _ => unreachable!(),
    }
    Ok(())
}
