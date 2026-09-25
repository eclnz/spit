use std::process::Command;

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
