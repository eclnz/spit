//! The command line: one command per step, with the files it works on given
//! as arguments.

mod support;

use std::fs;
use std::process::{Command, Output};

use support::Tree;

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

#[test]
fn help_lists_each_step_and_each_command_explains_itself() {
    for args in [&[][..], &["help"], &["--help"], &["-h"]] {
        let help = spit(args);
        assert!(help.status.success());
        let text = stdout(&help);
        for command in ["check", "inputs", "dag", "artifacts"] {
            assert!(text.contains(&format!("  {command} ")), "{text}");
        }
        assert!(text.contains(".spitdag"), "{text}");
    }
    for args in [&["help", "dag"][..], &["dag", "--help"]] {
        let text = stdout(&spit(args));
        assert!(
            text.contains(
                "usage: spit dag <recipe.spitin> or <pipeline.spit> <inputs.spitout | ->"
            ),
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
    for (example, expected_jobs) in [
        ("examples/pipelines/branching", 21),
        ("examples/pipelines/complex", 25),
        ("examples/pipelines/rich_shapes", 17),
        ("examples/commands/field_survey", 93),
        ("examples/analytics/analytics", 34),
    ] {
        let (pipeline, sources) = (format!("{example}.spit"), format!("{example}.spitout"));
        let output = spit(&["dag", &pipeline, &sources]);
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
        "artifacts",
        "examples/commands/command_demo.spit",
        "examples/commands/command_demo.spitout",
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
    let check = spit(&["check", "examples/commands/command_demo.spit", "--json"]);
    assert!(check.status.success());
    assert_eq!(stdout(&check), "{\"diagnostics\":[]}\n");
    let run = || {
        spit(&[
            "dag",
            "examples/commands/command_demo.spit",
            "examples/commands/command_demo.spitout",
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
    assert!(graph.contains("\"product\":\"shard\",\"entities\":{\"group\":\"alpha\",\"part\":\"01\"},\"type\":{\"name\":\"Lines\",\"args\":[]}"));
    assert!(graph.contains("\"inputs\":{\"items\":["));
    assert!(graph.contains("\"depends_on\":[1,2]"));
    assert_eq!(graph.matches("\"operation\":").count(), 5);
    assert_eq!(graph, stdout(&run()));
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
    let tree = Tree::new("cli-paths", &[]);
    let directory = tree.path();
    let file = directory.join("spit paths.spit");
    let pipeline =
        "source raw [id]\npath raw: input/{id}.txt\noperation copy(one)\nresult = copy(raw)\n";
    fs::write(&file, pipeline).unwrap();
    let missing = spit(&["check", file.to_str().unwrap(), "--path-rules"]);
    assert!(stdout(&missing).contains("result (output): MISSING"));

    fs::write(&file, format!("{pipeline}path result: output/{{id}}.txt\n")).unwrap();
    let complete = spit(&["check", file.to_str().unwrap(), "--strict-paths"]);
    assert!(complete.status.success(), "{}", stderr(&complete));
}

#[test]
fn check_prints_every_diagnostic_and_fails_only_on_errors() {
    let tree = Tree::new("cli-diagnostics", &[]);
    let directory = tree.path();
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
        stderr(&warned_dag).starts_with("error: dag needs a pipeline before"),
        "{}",
        stderr(&warned_dag)
    );
}

#[test]
fn a_recipe_names_its_own_pipeline_for_dag_and_artifacts() {
    let recipe = "examples/commands/command_demo.spitin";
    let pipeline = "examples/commands/command_demo.spit";
    let inventory = "examples/commands/command_demo.spitout";
    for command in ["dag", "artifacts"] {
        let alone = spit(&[command, recipe]);
        assert!(alone.status.success(), "{}", stderr(&alone));
        // The pipeline is the recipe's to name, not the command line's, even
        // when it names the same one.
        let named = spit(&[command, pipeline, recipe]);
        assert!(!named.status.success(), "{command}");
        assert_eq!(
            stderr(&named),
            format!(
                "error: `{recipe}` names its own pipeline; run `spit {command} {recipe}` without `{pipeline}`\n"
            )
        );
        // A .spitout names none, so it takes the pipeline.
        let settled = spit(&[command, pipeline, inventory]);
        assert!(settled.status.success(), "{}", stderr(&settled));
    }
    let spitout = spit(&["dag", inventory]);
    assert!(!spitout.status.success());
    assert!(
        stderr(&spitout).starts_with(
            "error: dag needs a pipeline before `examples/commands/command_demo.spitout`"
        ),
        "{}",
        stderr(&spitout)
    );
}
