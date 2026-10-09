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
        assert!(!text.contains('╪'));
        assert!(text.contains("Stage:"));
        assert!(text.contains("(preprocess) ──[session_b0]──> (anatomy)"));
        assert!(text.lines().all(|line| line.chars().count() <= 100));
    }
}
