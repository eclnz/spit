mod support;
use support::spit;

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
