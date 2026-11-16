//! An output with a naming suffix, whose tool is given a prefix and writes
//! files named after it, never one at the prefix itself.

mod support;

use support::{spit, text, Tree};

const FAST: &str = "\
source brain : Image .nii.gz [sub]
path brain: in/{sub}
check nifti_ok: test -s {@path}
default check .nii.gz: nifti_ok
operation fast(brain: Image) -> (seg: Image .nii.gz suffix \"_seg\", restore: Image .nii.gz beside seg suffix \"_restore\", pve: Image .nii.gz beside seg suffix \"_pve_0\")
command fast: tool -o {seg.prefix} {brain}
default path: out/{@product}/{sub}
seg, restore, pve = fast(brain)
";

fn dag(pipeline: &str) -> String {
    let tree = Tree::new("prefix-outputs", &[]);
    let file = tree.write("p.spit", pipeline);
    let records = tree.write("p.spitout", "sources:\n    brain[sub=01]\n");
    let output = spit(&[
        "dag",
        file.to_str().unwrap(),
        records.to_str().unwrap(),
        "--commands",
        "--paths",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

#[test]
fn a_tool_is_given_the_prefix_and_each_declared_file_is_named_after_it() {
    let shown = dag(FAST);
    for path in [
        "out/seg/01_seg.nii.gz",
        "out/seg/01_restore.nii.gz",
        "out/seg/01_pve_0.nii.gz",
    ] {
        assert!(shown.contains(&format!("path: {path}")), "{shown}");
    }
    assert!(
        shown.contains("run:    tool -o out/seg/01 in/01.nii.gz"),
        "{shown}"
    );
}

#[test]
fn each_declared_file_runs_the_checks_of_its_extension() {
    let shown = dag(FAST);
    let checks: Vec<&str> = shown
        .lines()
        .filter(|line| line.contains("check:"))
        .collect();
    assert_eq!(checks.len(), 4, "{shown}");
    assert!(shown.contains("test -s out/seg/01_pve_0.nii.gz"), "{shown}");
}

#[test]
fn a_rule_ending_in_the_naming_suffix_is_kept_and_one_that_skips_it_is_an_error() {
    let kept = FAST.replace(
        "default path: out/{@product}/{sub}\n",
        "default path: out/{@product}/{sub}\npath seg: out/seg/{sub}_seg.nii.gz\n",
    );
    assert!(dag(&kept).contains("run:    tool -o out/seg/01 in/01.nii.gz"));
    let tree = Tree::new("prefix-outputs-rule", &[]);
    let skipped = FAST.replace(
        "default path: out/{@product}/{sub}\n",
        "default path: out/{@product}/{sub}\npath seg: out/seg/{sub}.nii.gz\n",
    );
    let file = tree.write("p.spit", &skipped);
    let output = spit(&["check", file.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("writes it with a naming suffix; drop the extension"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_prefix_needs_a_naming_suffix_and_an_output() {
    let tree = Tree::new("prefix-outputs-errors", &[]);
    for (pipeline, says) in [
        (
            FAST.replace("seg: Image .nii.gz suffix \"_seg\"", "seg: Image .nii.gz"),
            "declares no naming suffix for `seg`",
        ),
        (
            FAST.replace("{seg.prefix} {brain}", "{seg.prefix} {brain.prefix}"),
            "only an operation's outputs give",
        ),
        (
            FAST.replace(
                "seg: Image .nii.gz suffix \"_seg\"",
                "seg: Image suffix \"_seg\"",
            ),
            "needs a file extension",
        ),
    ] {
        let file = tree.write("p.spit", &pipeline);
        let output = spit(&["check", file.to_str().unwrap()]);
        assert!(!output.status.success(), "{says}");
        assert!(
            text(&output.stderr).contains(says),
            "{says}: {}",
            text(&output.stderr)
        );
    }
}
