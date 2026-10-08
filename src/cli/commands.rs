//! The four commands, one step each: `check`, `inputs`, `dag` and
//! `artifacts`.

use std::error::Error;
use std::path::Path;

use spit::{
    bind_dag, bind_dag_with, diagnose_checked, diagnose_inputs, diagnose_recipe, inspect_paths,
    parse_input_spec_at, render_artifacts, render_artifacts_by_target, render_bound_dag,
    render_call, render_calls, render_calls_check_json, render_calls_json, render_check_json,
    render_dag, render_diagnostics_json, render_editor_json, render_source_inventory,
    render_step_counts, render_words_json, resolve_artifacts_partial, unused_sources_summary,
    validate_bound_source_files, validate_source_files, BoundDag, BoundPaths, Context, Diagnostic,
    FileNames, Gap, LeftOut, View,
};

use super::args::{CliArgs, Flag};
use super::load::{load_inputs, prepare, recorded_root, require_complete, settle};
use super::output::{
    is_inputs, is_recipe, job_count, passed, read_file, read_stdin, report, write_output,
    write_spitdag, Reported,
};

fn print_check_json(json: &str, diagnostics: &[Diagnostic]) -> Result<(), Box<dyn Error>> {
    print!("{json}");
    if diagnostics.iter().any(Diagnostic::is_error) {
        Err(Box::new(Reported))
    } else {
        Ok(())
    }
}

/// Step 1: compile a pipeline, or check a recipe against the pipeline it
/// names. Reads no data.
pub(crate) fn check(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let file = &args.file;
    let path = Path::new(file);
    let text = if args.has(Flag::Stdin) {
        read_stdin()?
    } else {
        read_file(file)?
    };
    if args.has(Flag::Calls) && (is_inputs(file) || is_recipe(file)) {
        return Err("--calls lists a pipeline's calls; give a .spit pipeline".into());
    }
    if is_inputs(file) {
        if args.has(Flag::PathRules) {
            return Err(
                "a .spitout has no path rules to show; check its recipe or pipeline".into(),
            );
        }
        let diagnostics = diagnose_inputs(&text);
        if args.has(Flag::Json) {
            let json = if args.has(Flag::Hovers) {
                render_words_json(&diagnostics, &text, Some(&text))
            } else {
                render_diagnostics_json(&diagnostics, &text, Some(&text))
            };
            return print_check_json(&json, &diagnostics);
        }
        report(&diagnostics, &text, Some(&text), FileNames::default())?;
        println!("Inputs valid.");
        return Ok(());
    }
    if is_recipe(file) {
        let diagnostics = diagnose_recipe(&text, path);
        if args.has(Flag::Json) {
            let json = if args.has(Flag::Hovers) {
                render_words_json(&diagnostics, &text, None)
            } else {
                render_diagnostics_json(&diagnostics, &text, None)
            };
            return print_check_json(&json, &diagnostics);
        }
        report(&diagnostics, &text, None, FileNames::default())?;
        if args.has(Flag::PathRules) {
            let recipe = parse_input_spec_at(&text, path)?;
            let pipeline_file = recipe
                .pipeline
                .as_ref()
                .expect("a checked recipe names its pipeline");
            let pipeline_text = read_file(&pipeline_file.display().to_string())?;
            let checked = diagnose_checked(&pipeline_text, Context::at(pipeline_file))
                .expect("a checked recipe has a valid pipeline");
            let mut merged = checked.pipeline;
            let defaulted: Vec<String> = recipe
                .rules
                .defaulted_sources(&merged)
                .into_iter()
                .map(str::to_owned)
                .collect();
            let named: Vec<String> = recipe
                .rules
                .named_source_paths(&merged)
                .keys()
                .cloned()
                .collect();
            let source_paths = recipe.rules.source_paths_for(&merged).into_owned();
            merged.product_paths.extend(source_paths);
            let coverage = inspect_paths(&merged)?
                .with_recipe_paths(named.iter().map(String::as_str))
                .with_recipe_default(defaulted.iter().map(String::as_str));
            println!("{coverage}");
        }
        println!("Recipe valid.");
        return Ok(());
    }
    let diagnosis = diagnose_checked(&text, Context::at(path));
    if args.has(Flag::Json) {
        // Paths are shown only for a pipeline that checks clean.
        let (diagnostics, paths) = match &diagnosis {
            Ok(checked) => (checked.warnings.as_slice(), Some(checked.paths.as_slice())),
            Err(all) => (all.as_slice(), None),
        };
        let json = if args.has(Flag::Calls) {
            match &diagnosis {
                Ok(checked) => {
                    let calls = render_calls_json(&checked.pipeline);
                    render_calls_check_json(diagnostics, &text, &calls)
                }
                Err(_) => render_diagnostics_json(diagnostics, &text, None),
            }
        } else if args.has(Flag::Hovers) {
            render_editor_json(diagnostics, &text, path, paths.unwrap_or_default())
        } else if let Some(paths) = paths {
            render_check_json(diagnostics, &text, paths)
        } else {
            render_diagnostics_json(diagnostics, &text, None)
        };
        return print_check_json(&json, diagnostics);
    }
    let checked = passed(
        diagnosis,
        |checked| &checked.warnings,
        &text,
        None,
        FileNames::default(),
    )?;
    if args.has(Flag::PathRules) {
        println!("{}", inspect_paths(&checked.pipeline)?);
    }
    if args.has(Flag::Calls) {
        print!("{}", render_calls(&checked.pipeline));
    }
    println!("Pipeline valid.");
    Ok(())
}

/// Step 2: settle a dataset from a recipe and write its `.spitout`.
pub(crate) fn inputs(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let loaded = load_inputs(args)?;
    report(
        &loaded.checked.warnings,
        &loaded.pipeline_text,
        None,
        loaded.names(),
    )?;
    let (mut settled, root) = settle(&loaded)?;
    if args.has(Flag::Unmatched) {
        for file in &settled.unmatched_files {
            println!("{file}");
        }
        return Ok(());
    }
    require_complete(&settled, &loaded.checked.pipeline)?;
    // A written .spitout records the dataset root, relative to the file, so
    // the two can move together. A printed one records none, as where it
    // will be kept is unknown.
    settled.inventory.root = args
        .value(Flag::Output)
        .map(|file| recorded_root(&root, Path::new(&file)));
    let text = render_source_inventory(
        &settled.inventory,
        &loaded.checked.pipeline,
        &loaded.recipe.rules,
    );
    write_output(args, &text, "the .spitout")
}

/// Step 3: resolve the jobs and print them, or write the `.spitdag`.
pub(crate) fn dag(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let mut prepared = prepare(args)?;
    if args.has(Flag::Partial) {
        prepared.report = resolve_artifacts_partial(
            &prepared.pipeline,
            &prepared.inputs.dag_inventory(),
            &prepared.inputs.unavailable(),
        )?;
        prepared
            .report
            .dag
            .locate_sources(&prepared.inputs.inventory);
    } else {
        require_complete(&prepared.inputs, &prepared.pipeline)?;
    }
    let dag = &prepared.report.dag;
    // Paths bound to check the source files are bound for the DAG too.
    let mut paths = None;
    if let Some(root) = &prepared.root {
        let (verified, bound) = validate_bound_source_files(&prepared.pipeline, dag, root)?;
        eprintln!("note: {verified}");
        paths = Some(bound);
    }
    eprintln!("note: {}", job_count(&prepared.pipeline, dag));
    if args.has(Flag::Partial) {
        let left_out: usize = prepared
            .report
            .incomplete
            .iter()
            .map(|job| job.outputs.len())
            .sum();
        eprintln!("note: planned {} jobs; left out {left_out} artifacts that cannot be produced (see left_out)", dag.jobs.len());
    }
    if let Some(unused) = unused_sources_summary(&prepared.report) {
        eprintln!("note: {unused}; `spit artifacts` lists them");
    }
    let bind = |paths: Option<BoundPaths>| match paths {
        Some(paths) => bind_dag_with(&prepared.pipeline, dag, paths),
        None => bind_dag(&prepared.pipeline, dag),
    };
    if args.has(Flag::Counts) {
        print!("{}", render_step_counts(&prepared.pipeline, dag));
    }
    let view = View {
        paths: args.has(Flag::Paths),
        commands: args.has(Flag::Commands),
    };
    // Print the jobs as `view` shows them, after the counts if there are any.
    let print_view = |bound: &BoundDag, view: View| {
        if view.commands {
            if let Some(root) = &prepared.root {
                eprintln!("note: commands run from `{}`", root.display());
            }
            if bound.jobs.iter().all(|job| job.command.is_none()) && !bound.jobs.is_empty() {
                eprintln!(
                    "note: no job has a command; `dag --jobs` lists each job's inputs and outputs"
                );
            }
        }
        if args.has(Flag::Counts) {
            println!();
        }
        print!("{}", render_bound_dag(bound, view));
    };
    if args.has(Flag::Output) || args.has(Flag::Json) {
        let mut bound = bind(paths)?;
        bound.removed = prepared.inputs.inventory.removed.clone();
        bound.left_out = prepared
            .report
            .incomplete
            .iter()
            .flat_map(|job| {
                let call = job.call.map(|call| prepared.pipeline.written_call(call));
                job.outputs.iter().map(move |artifact| LeftOut {
                    artifact: artifact.clone(),
                    reasons: job
                        .gaps
                        .iter()
                        .map(|gap| {
                            let reason = match gap {
                                Gap::Unmatched(error) => error.to_string(),
                                Gap::Blocked { port, artifact } => format!(
                                    "input `{port}` needs {artifact}, which cannot be produced"
                                ),
                            };
                            match call {
                                Some(call) => format!("in `{}`: {reason}", render_call(call)),
                                None => reason,
                            }
                        })
                        .collect(),
                })
            })
            .collect();
        bound.root = prepared.root.as_deref().map(|root| {
            std::path::absolute(root)
                .unwrap_or_else(|_| root.to_path_buf())
                .to_string_lossy()
                .into_owned()
        });
        write_spitdag(args, &bound)?;
        // With -o, the .spitdag goes to its file and the commands to stdout.
        if view.commands {
            print_view(&bound, view);
        }
        return Ok(());
    }
    if args.has(Flag::Jobs) {
        if args.has(Flag::Counts) {
            println!();
        }
        print!("{}", render_dag(dag));
    } else if view.paths || view.commands || !args.has(Flag::Counts) {
        // Plain `dag` shows the commands, as `--commands` does.
        let view = View {
            commands: view.commands || !view.paths,
            ..view
        };
        print_view(&bind(paths)?, view);
    }
    Ok(())
}

/// Step 3: what can be made, what cannot, and why.
pub(crate) fn artifacts(args: &CliArgs) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(args)?;
    let mut report = prepared.report;
    report.coverage = prepared.inputs.gaps;
    if let Some(root) = &prepared.root {
        validate_source_files(&prepared.pipeline, &report.dag, root)?;
    }
    let text = if args.has(Flag::ByTarget) {
        render_artifacts_by_target(&prepared.pipeline, &report)
    } else {
        render_artifacts(&prepared.pipeline, &report)
    };
    print!("{text}");
    Ok(())
}
