mod support;
use support::{spit, Tree};

#[test]
fn topology_needs_no_inventory_and_alias_matches() {
    let file = "examples/commands/command_demo/command_demo.spit";
    let tree = spit(&["dag", file, "--tree"]);
    assert!(
        tree.status.success(),
        "{}",
        String::from_utf8_lossy(&tree.stderr)
    );
    assert!(String::from_utf8_lossy(&tree.stdout).contains("(sort_lines)"));
    let ascii = spit(&["dag", file, "--ascii"]);
    assert!(ascii.status.success());
    assert_eq!(tree.stdout, ascii.stdout);
    for extra in [
        "--root",
        "--json",
        "--counts",
        "--paths",
        "--partial",
        "--commands",
        "--jobs",
        "-o",
    ] {
        let result = spit(&["dag", file, "--tree", extra, "unused"]);
        assert!(!result.status.success(), "{extra}");
    }
    let extra = spit(&["dag", file, "unused.spitout", "--tree"]);
    assert!(!extra.status.success());
    let wrong = spit(&["dag", "unused.spitin", "--tree"]);
    assert!(!wrong.status.success());
}

#[test]
fn reusable_and_nested_operations_have_one_component_diagram() {
    let tree = Tree::new("topology-components", &[]);
    tree.write("pipe.spit", "source raw : T [sub]\noperation leaf(x: T) -> T\noperation clean(x: T) -> (result: T):\n    mid = leaf(x)\n    result = leaf(mid)\noperation wrapper(x: T) -> (result: T):\n    result = clean(x)\na = wrapper(raw)\nb = wrapper(a)\n");
    let run = spit(&[
        "dag",
        tree.path().join("pipe.spit").to_str().unwrap(),
        "--tree",
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8(run.stdout).unwrap();
    assert_eq!(text.matches("(wrapper)").count(), 2);
    assert_eq!(text.matches("Component: wrapper").count(), 1);
    assert_eq!(text.matches("Component: clean").count(), 1);
    assert_eq!(text.matches("(leaf)").count(), 2);
    assert!(!text.contains("a::"));
}

#[test]
fn mrtrix_examples_render_narrow_stage_panels() {
    for file in [
        "examples/commands/mrtrix3_act/mrtrix3_act.spit",
        "examples/composites/mrtrix/act.spit",
    ] {
        let run = spit(&["dag", file, "--tree"]);
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        let text = String::from_utf8(run.stdout).unwrap();
        assert!(text.contains("[raw_dwi]"));
        assert!(text.contains('▼'));
        assert!(text.contains('┬'));
        assert!(!text.contains("see above"));
        assert!(text.contains("Stage:"));
        assert!(text.contains("(preprocess) ──[session_b0]──> (anatomy)"));
        assert!(text.lines().all(|line| line.chars().count() <= 120));
    }
}

#[test]
fn combined_tractography_connects_shared_outputs_once() {
    let tree = Tree::new("topology-connected-tractography", &[]);
    let source = std::fs::read_to_string("examples/commands/mrtrix3_act/mrtrix3_act.spit").unwrap();
    let mut inside = false;
    let mut combined = String::new();
    for line in source.lines() {
        if line == "stage tractography:" {
            inside = true;
        }
        if inside && line.starts_with("    stage ") {
            continue;
        }
        combined.push_str(if inside && line.starts_with("        ") {
            &line[4..]
        } else {
            line
        });
        combined.push('\n');
    }
    tree.write("pipeline.spit", &combined);
    let run = spit(&[
        "dag",
        tree.path().join("pipeline.spit").to_str().unwrap(),
        "--tree",
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8(run.stdout).unwrap();
    let stage = text.split("Stage: tractography\n").nth(1).unwrap();
    for product in [
        "corrected_dwi",
        "wm_response",
        "wm_fod",
        "five_tt",
        "act_tracks",
        "sift2_weights",
        "weighted_connectome",
    ] {
        assert_eq!(
            stage.matches(&format!("[{product}]")).count(),
            1,
            "{product}"
        );
    }
    assert_eq!(stage.matches('▼').count(), 13);
    assert!(stage.contains('╪'));
    assert!(stage
        .lines()
        .any(|line| line.contains("estimate_responses") && line.contains("estimate_fods")));
    assert!(stage
        .lines()
        .any(|line| line.contains("track_act") && line.contains("weight_streamlines")));
    assert!(!stage.trim().contains("\n\n"));
    assert!(stage.lines().all(|line| line.chars().count() <= 120));
}
