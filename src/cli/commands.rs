//! The four commands, one step each: `check`, `inputs`, `dag` and
//! `artifacts`.

use std::error::Error;
use std::path::Path;

use spit::{
    bind_dag, bind_dag_with, diagnose_checked, diagnose_inputs, diagnose_recipe, inspect_paths,
    parse_input_spec_at, render_artifacts, render_bound_dag, render_check_json, render_dag,
    render_diagnostics_json, render_editor_json, render_source_inventory, render_step_counts,
    render_words_json, resolve_artifacts_partial, unused_sources_summary,
    validate_bound_source_files, validate_source_files, BoundPaths, Context, FileNames, Gap,
    LeftOut, View,
};

use super::args::{CliArgs, Flag};
use super::load::{load_inputs, prepare, recorded_root, require_complete, settle};
use super::output::{
    is_inputs, is_recipe, job_count, passed, read_file, read_stdin, report, write_output,
    write_spitdag,
};
use super::suggest::suggest;

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
            print!("{json}");
            return Ok(());
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
            print!("{json}");
            return Ok(());
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
        let json = if args.has(Flag::Hovers) {
            render_editor_json(diagnostics, &text, path, paths.unwrap_or_default())
        } else if let Some(paths) = paths {
            render_check_json(diagnostics, &text, paths)
        } else {
            render_diagnostics_json(diagnostics, &text, None)
        };
        print!("{json}");
        return Ok(());
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
    if args.has(Flag::Suggest) {
        return suggest(&loaded, &args.file);
    }
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
        print!("{}", render_step_counts(dag));
    }
    if args.has(Flag::Output) || args.has(Flag::Json) {
        let mut bound = bind(paths)?;
        bound.removed = prepared.inputs.inventory.removed.clone();
        bound.left_out = prepared
            .report
            .incomplete
            .iter()
            .flat_map(|job| {
                job.outputs.iter().map(|artifact| LeftOut {
                    artifact: artifact.clone(),
                    reasons: job
                        .gaps
                        .iter()
                        .map(|gap| match gap {
                            Gap::Unmatched(error) => error.to_string(),
                            Gap::Blocked { port, artifact } => {
                                format!("input `{port}` needs {artifact}, which cannot be produced")
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
        return write_spitdag(args, &bound);
    }
    let view = View {
        paths: args.has(Flag::Paths),
        commands: args.has(Flag::Commands),
    };
    if view.paths || view.commands {
        if view.commands {
            if let Some(root) = &prepared.root {
                eprintln!("note: commands run from `{}`", root.display());
            }
        }
        // The counts come first, a blank line from the jobs.
        if args.has(Flag::Counts) {
            println!();
        }
        print!("{}", render_bound_dag(&bind(paths)?, view));
    } else if !args.has(Flag::Counts) {
        print!("{}", render_dag(dag));
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
    print!("{}", render_artifacts(&report));
    Ok(())
}
