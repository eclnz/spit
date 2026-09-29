//! The command line: one command per step, with the files it works on given
//! as arguments.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn spit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A fresh folder under the system's temporary directory.
fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("spit-cli-{name}-{nanos}"));
    fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn help_lists_each_step_and_each_command_explains_itself() {
    for args in [&[][..], &["help"], &["--help"], &["-h"]] {
        let help = spit(args);
        assert!(help.status.success());
        let text = stdout(&help);
        for command in ["check", "inputs", "dag", "artifacts", "bash"] {
            assert!(text.contains(&format!("  {command} ")), "{text}");
        }
        assert!(text.contains(".spitdag"), "{text}");
    }
    for args in [&["help", "dag"][..], &["dag", "--help"]] {
        let text = stdout(&spit(args));
        assert!(
            text.contains("usage: spit dag <pipeline.spit> <inputs.spitout | recipe.spitin | ->"),
            "{text}"
        );
        assert!(text.contains("-o <file>"), "{text}");
        assert!(text.contains("example:"), "{text}");
    }
    let version = spit(&["--version"]);
    assert_eq!(
        stdout(&version),
        format!("spit {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn removed_options_say_what_replaced_them() {
    let sources = spit(&[
        "dag",
        "examples/commands/bash_demo.spit",
        "--sources",
        "examples/commands/bash_demo.spitout",
    ]);
    assert!(!sources.status.success());
    assert!(stderr(&sources).contains("give the .spitout as a file after the pipeline"));
    let paths = spit(&["check", "examples/commands/bash_demo.spit", "--paths"]);
    assert!(stderr(&paths).contains("use --path-rules"));
    let stage = spit(&["dag", "a.spit", "b.spitout", "--stage", "x"]);
    assert!(stderr(&stage).starts_with("error: --stage applies to bash\n"));
}

#[test]
fn example_runs_with_embedded_inventory() {
    let output = spit(&["dag", "examples/analytics/analytics.spit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let dag = stdout(&output);
    assert_eq!(dag.matches("Job ").count(), 34);
    assert!(dag.contains("tenant_metrics[tenant=acme]"));
}

#[test]
fn check_compiles_the_pipeline_without_its_inputs() {
    let output = spit(&["check", "examples/types/typed.spit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "Pipeline valid.\n");
}

#[test]
fn dag_resolves_a_pipeline_over_a_spitout() {
    let output = spit(&[
        "dag",
        "examples/types/typed.spit",
        "examples/types/typed.spitout",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).starts_with("Job 1\n"));
}

#[test]
#[ignore = "the Bash backend is paused"]
fn bash_command_expands_observed_groups() {
    let output = spit(&[
        "bash",
        "examples/commands/bash_demo.spit",
        "examples/commands/bash_demo.spitout",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let script = stdout(&output);
    assert_eq!(script.matches("# Job ").count(), 5);
    assert!(script.contains("input/alpha/01.txt"));
    assert!(script.contains("input/beta/01.txt"));
}

#[test]
#[ignore = "the Bash backend is paused"]
fn a_spitdag_written_by_dag_is_what_bash_reads() {
    let directory = scratch("spitdag");
    let plan = directory.join("bash_demo.spitdag");
    let written = spit(&[
        "dag",
        "examples/commands/bash_demo.spit",
        "examples/commands/bash_demo.spitout",
        "-o",
        plan.to_str().unwrap(),
    ]);
    assert!(written.status.success(), "{}", stderr(&written));
    assert_eq!(stdout(&written), "");
    assert!(stderr(&written).contains("note: wrote the .spitdag to"));
    let from_plan = spit(&["bash", plan.to_str().unwrap()]);
    let direct = spit(&[
        "bash",
        "examples/commands/bash_demo.spit",
        "examples/commands/bash_demo.spitout",
    ]);
    fs::remove_dir_all(&directory).unwrap();
    assert!(from_plan.status.success(), "{}", stderr(&from_plan));
    assert_eq!(stdout(&from_plan), stdout(&direct));
}

#[test]
fn dag_with_paths_displays_resolved_paths_before_command_expansion() {
    let output = spit(&[
        "dag",
        "examples/commands/field_survey.spit",
        "examples/commands/field_survey.spitout",
        "--paths",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let report = stdout(&output);
    assert_eq!(report.matches("Job ").count(), 93);
    assert!(report.contains("moving: ground_map[site=01,visit=01]"));
    assert!(report.contains("path: site-01/visit-01/map/site-01_visit-01_map.tif"));
}

#[test]
fn expanded_examples_resolve() {
    for (pipeline, sources, expected_jobs) in [
        ("examples/pipelines/branching.spit", None, 21),
        ("examples/pipelines/complex.spit", None, 25),
        (
            "examples/pipelines/rich_shapes.spit",
            Some("examples/pipelines/rich_shapes.spitout"),
            17,
        ),
        (
            "examples/commands/field_survey.spit",
            Some("examples/commands/field_survey.spitout"),
            93,
        ),
        ("examples/analytics/analytics.spit", None, 34),
    ] {
        let mut args = vec!["dag", pipeline];
        args.extend(sources);
        let output = spit(&args);
        assert!(output.status.success(), "{pipeline}: {}", stderr(&output));
        assert!(
            stderr(&output).contains(&format!("note: {expected_jobs} jobs resolved.")),
            "{pipeline} resolved an unexpected number of jobs"
        );
    }
}

#[test]
fn path_rules_report_fallbacks_and_strict_paths_reject_them() {
    let rules = spit(&[
        "check",
        "examples/commands/field_survey.spit",
        "--path-rules",
    ]);
    assert!(rules.status.success(), "{}", stderr(&rules));
    let report = stdout(&rules);
    assert!(report.contains("photo_response (output): explicit"));
    assert!(report.contains("vegetation (output): default"));

    for command in ["check", "dag"] {
        let mut args = vec![command, "examples/commands/field_survey.spit"];
        if command == "dag" {
            args.push("examples/commands/field_survey.spitout");
        }
        args.push("--strict-paths");
        let strict = spit(&args);
        assert!(!strict.status.success(), "{command}");
        assert!(
            stderr(&strict).contains("strict paths requires explicit rules"),
            "{command}"
        );
    }
}

#[test]
fn each_option_applies_to_its_commands() {
    let output = spit(&[
        "bash",
        "examples/commands/bash_demo.spit",
        "examples/commands/bash_demo.spitout",
        "--paths",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).starts_with("error: --paths applies to dag\n"));
    let conflict = spit(&["dag", "a.spit", "b.spitout", "--json", "-o", "x"]);
    assert!(stderr(&conflict).starts_with("error: --json cannot be used with -o\n"));
    let extra = spit(&["check", "a.spit", "b.spitout"]);
    assert!(stderr(&extra).starts_with("error: unexpected file `b.spitout`\n"));
}

#[test]
fn check_json_reads_the_pipeline_file_and_dag_json_emits_the_spitdag() {
    let check = spit(&["check", "examples/commands/bash_demo.spit", "--json"]);
    assert!(check.status.success());
    assert_eq!(stdout(&check), "{\"diagnostics\":[]}\n");
    let run = || {
        spit(&[
            "dag",
            "examples/commands/bash_demo.spit",
            "examples/commands/bash_demo.spitout",
            "--json",
        ])
    };
    let dag = run();
    assert!(dag.status.success(), "{}", stderr(&dag));
    let graph = stdout(&dag);
    assert!(graph.starts_with("{\"version\":2,\"external_inputs\":["));
    // A bound DAG: every artifact has its path and every job its command.
    assert!(graph.contains("\"path\":\"input/alpha/01.txt\""), "{graph}");
    assert!(graph.contains("\"command\":[[\"sort\"]"), "{graph}");
    assert_eq!(
        spit::BoundDag::from_json(&graph).unwrap().to_json(),
        graph,
        "a .spitdag reads back as written"
    );
    assert!(graph.contains("\"product\":\"shard\",\"entities\":{\"group\":\"alpha\",\"part\":\"01\"},\"type\":{\"name\":\"Lines\",\"args\":[]}"));
    assert!(graph.contains("\"inputs\":{\"items\":["));
    assert!(graph.contains("\"depends_on\":[1,2]"));
    assert_eq!(graph.matches("\"operation\":").count(), 5);
    assert_eq!(graph, stdout(&run()));
}

#[test]
#[ignore = "the Bash backend is paused"]
fn bash_stage_lists_earlier_outputs_as_required_inputs() {
    let directory = scratch("stage");
    let plan = directory.join("stages.spitdag");
    let written = spit(&[
        "dag",
        "examples/stages/stages.spit",
        "examples/commands/bash_demo.spitout",
        "-o",
        plan.to_str().unwrap(),
    ]);
    assert!(written.status.success(), "{}", stderr(&written));
    let dag = spit::BoundDag::from_json(&fs::read_to_string(&plan).unwrap()).unwrap();
    let analysis = dag.only_stage("analysis").unwrap();
    let external: Vec<_> = analysis
        .external_inputs()
        .into_iter()
        .map(|artifact| artifact.product.as_str())
        .collect();
    assert!(external.contains(&"merged"), "{external:?}");
    assert!(analysis.jobs.iter().all(|job| job.operation != "merge"));
    let script = spit(&["bash", plan.to_str().unwrap(), "--stage", "analysis"]);
    fs::remove_dir_all(&directory).unwrap();
    assert!(script.status.success(), "{}", stderr(&script));
    assert!(stdout(&script).contains("spit_require \"$SPIT_ROOT\"/'preprocess/merged/"));
}

#[test]
fn dag_json_names_every_output_port() {
    let output = spit(&[
        "dag",
        "examples/pipelines/selectors.spit",
        "examples/pipelines/selectors.spitout",
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let graph = stdout(&output);
    assert!(graph.contains("\"outputs\":{\"low\":{\"product\":\"low_band\""));
    assert!(graph.contains("\"high\":{\"product\":\"high_band\""));
}

#[test]
fn dag_json_stage_is_an_array_of_names() {
    let output = spit(&[
        "dag",
        "examples/stages/nested.spit",
        "examples/stages/nested.spitout",
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let graph = stdout(&output);
    assert!(graph.contains("\"stage\":[\"preprocess\",\"clean\"]"));
    assert!(!graph.contains("\"stage\":\"preprocess/clean\""));
}

#[test]
fn path_rules_show_a_missing_rule_and_strict_paths_accept_complete_rules() {
    let directory = scratch("paths");
    let file = directory.join("spit paths.spit");
    let pipeline =
        "source raw [id]\npath raw: input/{id}.txt\noperation copy(one)\nresult = copy(raw)\n";
    fs::write(&file, pipeline).unwrap();
    let missing = spit(&["check", file.to_str().unwrap(), "--path-rules"]);
    assert!(stdout(&missing).contains("result (output): MISSING"));

    fs::write(&file, format!("{pipeline}path result: output/{{id}}.txt\n")).unwrap();
    let complete = spit(&["check", file.to_str().unwrap(), "--strict-paths"]);
    fs::remove_dir_all(&directory).unwrap();
    assert!(complete.status.success(), "{}", stderr(&complete));
}

#[test]
fn check_prints_every_diagnostic_and_fails_only_on_errors() {
    let directory = scratch("diagnostics");
    let broken = directory.join("broken.spit");
    fs::write(
        &broken,
        "source raw [id, batch]\npath: {product}/{entities}.txt\npath raw: in/{id}.txt\noperation clean(one)\ncleaned = clean(rwa)\n",
    )
    .unwrap();
    let warned = directory.join("warned.spit");
    fs::write(
        &warned,
        "source raw [id]\nsource spare [id]\noperation clean(one)\ncleaned = clean(raw)\n",
    )
    .unwrap();
    let broken_check = spit(&["check", broken.to_str().unwrap()]);
    let warned_check = spit(&["check", warned.to_str().unwrap()]);
    let warned_dag = spit(&["dag", warned.to_str().unwrap()]);
    fs::remove_dir_all(&directory).unwrap();

    assert!(!broken_check.status.success());
    assert_eq!(
        stderr(&broken_check),
        "warning: line 1, column 8: source product `raw` is never used as an input\nerror: line 3, column 11: path template for `raw` omits dimension `batch`; artifacts differing only in `batch` would share a path\nerror: line 5, column 17: unknown product `rwa`\n"
    );

    // Check compiles the pipeline alone; dag needs the dataset's inputs.
    assert!(warned_check.status.success());
    assert_eq!(
        stderr(&warned_check),
        "warning: line 2, column 8: source product `spare` is never used as an input\n"
    );
    assert_eq!(stdout(&warned_check), "Pipeline valid.\n");
    assert!(!warned_dag.status.success());
    assert!(
        stderr(&warned_dag).contains("needs the dataset's inputs"),
        "{}",
        stderr(&warned_dag)
    );
}

#[test]
fn a_spitout_replaces_records_written_in_the_pipeline() {
    let directory = scratch("override");
    let pipeline = directory.join("pipeline.spit");
    fs::write(
        &pipeline,
        "source image [subject]\noperation f(Image) -> Image\nout = f(image)\nsources:\n  image[subject=a\n",
    )
    .unwrap();
    let inventory = directory.join("inventory.spitout");
    fs::write(&inventory, "sources:\n  image[subject=b]\n").unwrap();
    let result = spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        inventory.to_str().unwrap(),
    ]);
    fs::remove_dir_all(&directory).unwrap();
    assert!(result.status.success(), "{result:?}");
    assert!(stderr(&result).contains("note: 1 jobs resolved."));
    assert!(stderr(&result).contains("this inline inventory is ignored"));
}

#[test]
fn rules_and_records_in_a_pipeline_still_work_with_a_warning() {
    let output = spit(&["check", "examples/basic/basic.spit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("5 jobs resolved."));
    let warnings = stderr(&output);
    assert!(
        warnings.contains("`require` rules belong in a .spitin recipe"),
        "{warnings}"
    );
    assert!(
        warnings.contains("records belong in a .spitout"),
        "{warnings}"
    );
}
