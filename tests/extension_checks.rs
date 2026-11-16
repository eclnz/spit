//! `default check .ext:`: checks chosen by a file's whole extension, for
//! sources and outputs, with opt-outs and the scope of a stage.

mod support;

use support::{spit, text, Tree};

const CHECKS: &str = "\
check nifti_ok: test -s {@path}
check tidy: test -s {@path}
default path: out/{@product}/{sub}
path t1: in/{sub}
path raw: raw/{sub}
path meta: meta/{sub}
";

const SOURCES: &str = "\
source t1 : Image .nii.gz [sub]
source raw : Image .nii.gz [sub] @check(!nifti_ok)
source meta : Table .tsv [sub]
";

const RECORDS: &str = "sources:\n    t1[sub=01]\n    raw[sub=01]\n    meta[sub=01]\n";

/// The commands `dag --commands` prints for `pipeline` over the records.
fn commands(pipeline: &str) -> String {
    let tree = Tree::new("extension-checks", &[]);
    let file = tree.write("p.spit", pipeline);
    let records = tree.write("p.spitout", RECORDS);
    let output = spit(&[
        "dag",
        file.to_str().unwrap(),
        records.to_str().unwrap(),
        "--commands",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

fn lines_with<'a>(shown: &'a str, word: &str) -> Vec<&'a str> {
    shown
        .lines()
        .filter(|line| line.contains(word))
        .map(str::trim)
        .collect()
}

#[test]
fn sources_and_outputs_run_the_checks_of_their_extension() {
    let shown = commands(&format!(
        "{SOURCES}{CHECKS}default check .nii.gz: nifti_ok\ndefault check .tsv: tidy\n\
         operation bet(a: Image, b: Image, m: Table) -> (brain: Image .nii.gz, mask: Image \"_mask.nii.gz\" beside brain, table: Table .tsv, log: Text .txt)\n\
         command bet: tool {{a}} {{b}} {{m}} {{brain}} {{table}} {{log}}\n\
         brain, mask, table, log = bet(t1, raw, meta)\n"
    ));
    assert_eq!(
        lines_with(&shown, "check:"),
        [
            "check:  test -s in/01.nii.gz",
            "check:  test -s meta/01.tsv",
            "check:  test -s out/brain/01.nii.gz",
            "check:  test -s out/brain/01_mask.nii.gz",
            "check:  test -s out/table/01.tsv",
        ],
        "{shown}"
    );
}

#[test]
fn a_gz_file_is_not_a_nifti_and_a_folder_matches_no_extension() {
    let shown = commands(&format!(
        "source t1 : Image .nii.gz [sub]\nsource raw : Image .nii.gz [sub]\nsource meta : Table .tsv [sub]\n\
         {CHECKS}default check .nii.gz: nifti_ok\n\
         operation f(a: Image) -> (zip: Image .gz, dir: Image .nii.gz /)\n\
         command f: tool {{a}} {{zip}} {{dir}}\n\
         zip, dir = f(t1)\n"
    ));
    assert_eq!(
        lines_with(&shown, "check:"),
        ["check:  test -s in/01.nii.gz"],
        "{shown}"
    );
}

#[test]
fn a_stage_adds_to_the_file_and_an_output_opts_out() {
    let shown = commands(&format!(
        "{SOURCES}{CHECKS}default check .nii.gz: nifti_ok\n\
         operation f(a: Image) -> (x: Image .nii.gz, y: Image .nii.gz @check(!nifti_ok))\n\
         command f: tool {{a}} {{x}} {{y}}\n\
         stage s:\n    default check .nii.gz: !nifti_ok, tidy\n    x, y = f(t1)\n"
    ));
    assert!(shown.contains("test -s out/x/01"), "{shown}");
    assert_eq!(lines_with(&shown, "check:").len(), 3, "{shown}");
}

#[test]
fn one_list_for_an_extension_in_a_scope_and_only_declared_checks() {
    let tree = Tree::new("extension-checks-errors", &[]);
    for (rules, says) in [
        (
            "default check .tsv: tidy\ndefault check .tsv: tidy\n",
            "duplicate `default check .tsv:` list",
        ),
        ("default check .tsv: missing\n", "missing"),
        ("default check .tsv tidy\n", "expected `:`"),
    ] {
        let file = tree.write("p.spit", &format!("{SOURCES}{CHECKS}{rules}"));
        let output = spit(&["check", file.to_str().unwrap()]);
        assert!(!output.status.success(), "{rules}");
        assert!(
            text(&output.stderr).contains(says),
            "{rules}: {}",
            text(&output.stderr)
        );
    }
}
