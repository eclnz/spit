#![deny(clippy::unwrap_used)]

mod cli;

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::env;
use std::process::ExitCode;

use spit::{render_diagnostics_json, Diagnostic, DiagnosticSource, Severity};

use crate::cli::args::{parse_args, Command, Flag, Help, Request};
use crate::cli::commands::{artifacts, check, dag, inputs};
use crate::cli::output::Reported;

fn main() -> ExitCode {
    let args = match parse_args(env::args().skip(1)) {
        Ok(Request::Run(args)) => args,
        Ok(Request::Help(command)) => {
            print!("{}", Help(command));
            return ExitCode::SUCCESS;
        }
        Ok(Request::Keywords) => {
            print!("{}", spit::keywords_json());
            return ExitCode::SUCCESS;
        }
        Ok(Request::Version) => {
            println!(
                "SPIT (Simple Pipeline in Text) compiler, version {}",
                env!("CARGO_PKG_VERSION")
            );
            return ExitCode::SUCCESS;
        }
        Ok(Request::DagVersion) => {
            println!("{}", spit::SPITDAG_VERSION);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let diagnostics_json = args.has(Flag::Json) && args.command == Command::Check;
    let result = match args.command {
        Command::Check => check(&args),
        Command::Inputs => inputs(&args),
        Command::Dag => dag(&args),
        Command::Artifacts => artifacts(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.is::<Reported>() => ExitCode::FAILURE,
        Err(error) if diagnostics_json => {
            let diagnostic = Diagnostic {
                severity: Severity::Error,
                source: DiagnosticSource::Pipeline,
                line: None,
                columns: None,
                message: error.to_string(),
                code: "cli.failed".to_owned(),
                file: None,
                external_text: None,
                related: Vec::new(),
            };
            print!("{}", render_diagnostics_json(&[diagnostic], "", None));
            ExitCode::FAILURE
        }
        Err(error) => {
            if !error.is::<Reported>() {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}
