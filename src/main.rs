//! The `spit` command line. Each command is one step, and the files given
//! say what it works on:
//!
//! 1. `check` compiles a pipeline, or checks a recipe against its pipeline;
//! 2. `inputs` settles a dataset from a recipe, writing a `.spitout`;
//! 3. `dag` and `artifacts` resolve a pipeline's jobs over a `.spitout`.
//!
//! A command given files from an earlier step runs the steps between in
//! memory. Nothing is loaded that the command line does not name.

// Outside tests, `expect` states the invariant it relies on; see AGENTS.md.
#![deny(clippy::unwrap_used)]

mod cli;

/// SPIT makes and frees many small strings; mimalloc does this faster than
/// the system allocator.
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
        Ok(Request::Version) => {
            println!("spit {}", env!("CARGO_PKG_VERSION"));
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
        // Editors expect JSON even when the check cannot run.
        Err(error) if diagnostics_json => {
            let diagnostic = Diagnostic {
                severity: Severity::Error,
                source: DiagnosticSource::Pipeline,
                line: None,
                columns: None,
                message: error.to_string(),
                file: None,
                external_text: None,
                related: Vec::new(),
            };
            print!("{}", render_diagnostics_json(&[diagnostic], "", None));
            ExitCode::SUCCESS
        }
        Err(error) => {
            if !error.is::<Reported>() {
                eprintln!("error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}
