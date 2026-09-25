use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn example_runs_with_embedded_inventory() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["dag", "examples/basic.spit"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dag = String::from_utf8(output.stdout).unwrap();
    assert_eq!(dag.matches("Job ").count(), 5);
    assert!(dag.contains("mean_bold[sub=01,ses=01]"));
}

#[test]
fn separate_inventory_remains_supported() {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            "examples/typed.spit",
            "--sources",
            "examples/typed.sources",
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
            "examples/bash_demo.spit",
            "--sources",
            "examples/bash_demo.sources",
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
fn expanded_examples_resolve() {
    for (pipeline, sources, expected_jobs) in [
        ("examples/branching.spit", None, 21),
        ("examples/complex.spit", None, 25),
        (
            "examples/rich_shapes.spit",
            Some("examples/rich_shapes.sources"),
            17,
        ),
        (
            "examples/mrtrix3_act.spit",
            Some("examples/mrtrix3_act.sources"),
            83,
        ),
        ("examples/analytics.spit", None, 34),
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
            "examples/mrtrix3_act.spit",
            "--sources",
            "examples/mrtrix3_act.sources",
        ])
        .output()
        .unwrap();
    assert!(paths.status.success());
    let report = String::from_utf8(paths.stdout).unwrap();
    assert!(report.contains("wm_response (output): explicit"));
    assert!(report.contains("wm_fod (output): default"));

    let strict = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args([
            "check",
            "examples/mrtrix3_act.spit",
            "--strict-paths",
            "--sources",
            "examples/mrtrix3_act.sources",
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
