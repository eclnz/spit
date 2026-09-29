use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn example_runs_with_embedded_inventory() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["dag", "examples/analytics/analytics.spit"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dag = String::from_utf8(output.stdout).unwrap();
    assert_eq!(dag.matches("Job ").count(), 34);
    assert!(dag.contains("tenant_metrics[tenant=acme]"));
}

#[test]
fn separate_inventory_remains_supported() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            "examples/types/typed.spit",
            "--sources",
            "examples/types/typed.sources",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("Pipeline valid."));
}

#[test]
fn bash_command_expands_observed_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "bash",
            "examples/commands/bash_demo.spit",
            "--sources",
            "examples/commands/bash_demo.sources",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let script = String::from_utf8(output.stdout).unwrap();
    assert_eq!(script.matches("# Job ").count(), 5);
    assert!(script.contains("input/alpha/01.txt"));
    assert!(script.contains("input/beta/01.txt"));
}

#[test]
fn dag_with_paths_displays_resolved_paths_before_command_expansion() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/commands/field_survey.spit",
            "--sources",
            "examples/commands/field_survey.sources",
            "--paths",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8(output.stdout).unwrap();
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
            Some("examples/pipelines/rich_shapes.sources"),
            17,
        ),
        (
            "examples/commands/field_survey.spit",
            Some("examples/commands/field_survey.sources"),
            93,
        ),
        ("examples/analytics/analytics.spit", None, 34),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_spit"));
        command.args(["check", pipeline]);
        if let Some(sources) = sources {
            command.args(["--sources", sources]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{pipeline}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains(&format!("{expected_jobs} jobs resolved.")),
            "{pipeline} resolved an unexpected number of jobs"
        );
    }
}

#[test]
fn check_paths_reports_fallbacks_and_strict_check_rejects_them() {
    let paths = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            "examples/commands/field_survey.spit",
            "--sources",
            "examples/commands/field_survey.sources",
            "--paths",
        ])
        .output()
        .unwrap();
    assert!(paths.status.success());
    let report = String::from_utf8(paths.stdout).unwrap();
    assert!(report.contains("photo_response (output): explicit"));
    assert!(report.contains("vegetation (output): default"));

    let strict = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            "examples/commands/field_survey.spit",
            "--strict-paths",
            "--sources",
            "examples/commands/field_survey.sources",
        ])
        .output()
        .unwrap();
    assert!(!strict.status.success());
    assert!(String::from_utf8(strict.stderr)
        .unwrap()
        .contains("strict paths requires explicit rules"));
}

#[test]
fn paths_flag_applies_to_check_and_dag_only() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "bash",
            "examples/commands/bash_demo.spit",
            "--sources",
            "examples/commands/bash_demo.sources",
            "--paths",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: --paths applies to check and dag\n"
    );
}

#[test]
fn check_json_reads_the_pipeline_file_and_dag_json_emits_jobs() {
    let run = |command: &str| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args([
                command,
                "examples/commands/bash_demo.spit",
                "--sources",
                "examples/commands/bash_demo.sources",
                "--json",
            ])
            .output()
            .unwrap()
    };
    let check = run("check");
    assert!(check.status.success());
    assert_eq!(
        String::from_utf8(check.stdout).unwrap(),
        "{\"diagnostics\":[]}\n"
    );
    let dag = run("dag");
    assert!(dag.status.success());
    let graph = String::from_utf8(dag.stdout).unwrap();
    assert!(graph.starts_with("{\"version\":1,\"external_inputs\":["));
    assert!(graph.contains("\"product\":\"shard\",\"entities\":{\"group\":\"alpha\",\"part\":\"01\"},\"type\":{\"name\":\"Lines\",\"args\":[]}"));
    assert!(graph.contains("\"inputs\":{\"items\":["));
    assert!(graph.contains("\"depends_on\":[1,2]"));
    assert_eq!(graph.matches("\"operation\":").count(), 5);
    assert_eq!(graph, String::from_utf8(run("dag").stdout).unwrap());
}

#[test]
fn dag_json_stage_lists_earlier_outputs_as_external_inputs() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/stages/stages.spit",
            "--sources",
            "examples/commands/bash_demo.sources",
            "--stage",
            "analysis",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph = String::from_utf8(output.stdout).unwrap();
    assert!(graph.contains("\"external_inputs\":[{\"product\":\"merged\""));
    assert!(graph.contains("\"stage\":[\"analysis\"]"));
    assert!(!graph.contains("\"operation\":\"merge\""));
    assert!(graph.contains("\"depends_on\":[]"));
}

#[test]
fn dag_json_names_every_output_port() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/pipelines/selectors.spit",
            "--sources",
            "examples/pipelines/selectors.sources",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph = String::from_utf8(output.stdout).unwrap();
    assert!(graph.contains("\"outputs\":{\"low\":{\"product\":\"low_band\""));
    assert!(graph.contains("\"high\":{\"product\":\"high_band\""));
}

#[test]
fn dag_json_stage_is_an_array_of_names() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "dag",
            "examples/stages/nested.spit",
            "--sources",
            "examples/stages/nested.sources",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph = String::from_utf8(output.stdout).unwrap();
    assert!(graph.contains("\"stage\":[\"preprocess\",\"clean\"]"));
    assert!(!graph.contains("\"stage\":\"preprocess/clean\""));
}

#[test]
fn check_paths_fails_on_missing_rule_and_strict_check_accepts_complete_rules() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let file =
        std::env::temp_dir().join(format!("spit paths {} {suffix}.spit", std::process::id()));
    let pipeline = "source raw [id]\npath raw: input/{id}.txt\noperation copy(one)\nresult = copy(raw)\nsources:\n    raw[id=x]\n";
    fs::write(&file, pipeline).unwrap();
    let missing = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", file.to_str().unwrap(), "--paths"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8(missing.stdout)
        .unwrap()
        .contains("result (output): MISSING"));

    fs::write(
        &file,
        pipeline.replace("sources:", "path result: output/{id}.txt\nsources:"),
    )
    .unwrap();
    let complete = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", file.to_str().unwrap(), "--strict-paths"])
        .output()
        .unwrap();
    fs::remove_file(&file).unwrap();
    assert!(
        complete.status.success(),
        "{}",
        String::from_utf8_lossy(&complete.stderr)
    );
}

#[test]
fn check_prints_every_diagnostic_and_fails_only_on_errors() {
    let directory = std::env::temp_dir().join(format!(
        "spit-cli-diagnostics-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
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
    let run = |path: &std::path::Path, command: &str| {
        Command::new(env!("CARGO_BIN_EXE_spit"))
            .args([command, path.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let broken_check = run(&broken, "check");
    let warned_check = run(&warned, "check");
    let warned_dag = run(&warned, "dag");
    fs::remove_dir_all(&directory).unwrap();

    assert!(!broken_check.status.success());
    assert_eq!(
        String::from_utf8(broken_check.stderr).unwrap(),
        "warning: line 1, column 8: source product `raw` is never used as an input\nerror: line 3, column 11: path template for `raw` omits dimension `batch`; artifacts differing only in `batch` would share a path\nerror: line 5, column 17: unknown product `rwa`\n"
    );

    // Without an inventory, check stops after the pipeline checks; dag needs jobs.
    assert!(warned_check.status.success());
    assert_eq!(
        String::from_utf8(warned_check.stderr).unwrap(),
        "warning: line 2, column 8: source product `spare` is never used as an input\n"
    );
    assert!(String::from_utf8(warned_check.stdout)
        .unwrap()
        .contains("No source inventory; jobs not resolved."));
    assert!(!warned_dag.status.success());
    assert!(String::from_utf8(warned_dag.stderr)
        .unwrap()
        .ends_with("error: no inline source inventory; supply --sources <inventory.spit|->\n"));
}
