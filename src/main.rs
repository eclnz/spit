use std::env;
use std::error::Error;
use std::fs;
use std::process::ExitCode;

use spit::{parse_pipeline, render_dag, resolve};

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
    if args.len() != 2 || !matches!(args[0].as_str(), "check" | "dag") {
        return Err("usage: spit <check|dag> <pipeline.spit>".into());
    }
    let text = fs::read_to_string(&args[1])?;
    let pipeline = parse_pipeline(&text)?;
    let dag = resolve(&pipeline)?;
    match args[0].as_str() {
        "check" => println!("Pipeline valid.\n\n{} jobs resolved.", dag.jobs.len()),
        "dag" => print!("{}", render_dag(&dag)),
        _ => unreachable!(),
    }
    Ok(())
}
