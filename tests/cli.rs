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
fn bound_dag_displays_resolved_paths_before_command_expansion() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "bound-dag",
            "examples/commands/field_survey.spit",
            "--sources",
            "examples/commands/field_survey.sources",
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
fn paths_reports_fallbacks_and_strict_check_rejects_them() {
    let paths = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "paths",
            "examples/commands/field_survey.spit",
            "--sources",
            "examples/commands/field_survey.sources",
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
fn paths_fails_on_missing_rule_and_strict_check_accepts_complete_rules() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let file =
        std::env::temp_dir().join(format!("spit paths {} {suffix}.spit", std::process::id()));
    let pipeline = "source raw [id]\npath raw: input/{id}.txt\noperation copy(one)\nresult = copy(raw)\nsources:\n    raw[id=x]\n";
    fs::write(&file, pipeline).unwrap();
    let missing = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["paths", file.to_str().unwrap()])
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
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
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
