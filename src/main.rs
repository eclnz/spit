use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

use spit::{parse_pipeline, parse_source_inventory, render_dag, resolve};

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
    if args.len() != 4 || !matches!(args[0].as_str(), "check" | "dag") || args[2] != "--sources" {
        return Err("usage: spit <check|dag> <pipeline.spit> --sources <inventory.spit|->".into());
    }
    let pipeline = parse_pipeline(&fs::read_to_string(&args[1])?)?;
    let inventory_text = if args[3] == "-" {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text)?;
        text
    } else {
        fs::read_to_string(&args[3])?
    };
    let inventory = parse_source_inventory(&inventory_text)?;
    let dag = resolve(&pipeline, &inventory)?;
    match args[0].as_str() {
        "check" => println!("Pipeline valid.\n\n{} jobs resolved.", dag.jobs.len()),
        "dag" => print!("{}", render_dag(&dag)),
        _ => unreachable!(),
    }
    Ok(())
}
