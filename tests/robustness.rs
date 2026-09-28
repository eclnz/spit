//! Hostile and unusual input: odd file names under a root, paths that
//! cannot coexist, commands written as if for a shell, and the command line
//! used wrongly.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use spit::{
    diagnose, discover_source_files, parse_document, parse_pipeline, render_bash, resolve,
    Cardinality, Diagnostic,
};

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str, files: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!("spit-robust-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn spit(args: &[&str], stdin: Option<&[u8]>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.unwrap_or_default())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn rendered(diagnostics: &[Diagnostic], text: &str) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.display_in(text, None).to_string())
        .collect()
}

const ONE_SOURCE: &str = "source x [s]\npath x: in/{s}.txt\n";

#[test]
fn discovery_skips_values_spit_would_write_differently() {
    // `%41` decodes to `A`, but SPIT writes `A` as `A`: a script would look
    // for `in/A.txt`, not the file found.
    let tree = Tree::new(
        "canonical",
        &[
            "in/%41.txt",
            "in/%2e.txt",
            "in/x%zz.txt",
            "in/%2E%2E.txt",
            "in/b.txt",
        ],
    );
    let pipeline = parse_pipeline(ONE_SOURCE).unwrap();
    let discovery = discover_source_files(&pipeline, tree.path()).unwrap();
    let records: Vec<_> = discovery
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.to_string())
        .collect();
    assert_eq!(records, ["s=..", "s=b"]);
    assert_eq!(discovery.skipped.len(), 3, "{:?}", discovery.skipped);
    assert!(discovery.skipped[0].starts_with("`in/%2e.txt`"));
    assert!(discovery.skipped[1].contains("is not how SPIT writes a value"));
    assert!(discovery.skipped[2].contains("is not valid `%XX` text"));
}

#[test]
fn discovery_reports_skipped_files_and_still_succeeds() {
    let tree = Tree::new("skipped", &["in/%41.txt", "in/a.txt"]);
    let pipeline = tree.path().join("pipeline.spit");
    fs::write(&pipeline, ONE_SOURCE).unwrap();
    let output = spit(
        &[
            "discover",
            pipeline.to_str().unwrap(),
            "--root",
            tree.path().to_str().unwrap(),
        ],
        None,
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "sources:\n    x[s=a]\n");
    assert!(text(&output.stderr).contains("warning: skipped `in/%41.txt`"));
}

#[test]
fn discovery_is_fast_however_values_could_be_split() {
    // Adjacent placeholders, or separators values may hold, give a long
    // file name exponentially many ways to split.
    let long = format!("in/{}.txt", "a".repeat(200));
    let dashed = format!("in/{}z.txt", "a-".repeat(100));
    let tree = Tree::new("backtrack", &[&long, &dashed]);
    for rule in [
        "source x [a, b, c, d, e, f, g]\npath x: in/{a}{b}{c}{d}{e}{f}{g}.dat\n",
        "source x [a, b, c, d, e]\npath x: in/{a}-{b}-{c}-{d}-{e}.dat\n",
    ] {
        let started = Instant::now();
        let discovery = discover_source_files(&parse_pipeline(rule).unwrap(), tree.path()).unwrap();
        assert!(discovery.inventory.artifacts.is_empty());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
    }
    // A match still finds its values, including a repeated dimension.
    let tree = Tree::new("repeat", &["in/ab-x/ab.txt", "in/ab-x/cd.txt"]);
    let rule = "source x [s, t]\npath x: in/{s}-{t}/{s}.txt\n";
    let discovery = discover_source_files(&parse_pipeline(rule).unwrap(), tree.path()).unwrap();
    let records: Vec<_> = discovery
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.to_string())
        .collect();
    assert_eq!(records, ["s=ab,t=x"]);
}

#[cfg(unix)]
#[test]
fn discovery_follows_links_without_looping() {
    let tree = Tree::new("links", &["elsewhere/c.txt", "data/in/a.txt"]);
    let data = tree.path().join("data");
    std::os::unix::fs::symlink(tree.path().join("elsewhere/c.txt"), data.join("in/b.txt")).unwrap();
    std::os::unix::fs::symlink(&data, data.join("in/loop")).unwrap();
    let pipeline = parse_pipeline(ONE_SOURCE).unwrap();
    let discovery = discover_source_files(&pipeline, &data).unwrap();
    let records: Vec<_> = discovery
        .inventory
        .artifacts
        .iter()
        .map(|record| record.entities.to_string())
        .collect();
    assert_eq!(records, ["s=a", "s=b"]);
}

#[test]
fn a_path_inside_another_artifacts_file_is_rejected() {
    let text = "source x [s]\npath x: in/{s}.txt\noperation f(a) -> Text\ncommand f: cp {a} {output}\npath y: in/{s}.txt/out.txt\ny = f(x)\n";
    assert_eq!(
        rendered(&diagnose(text, None), text),
        ["error: line 5, column 9: path rule for `y` puts files inside `in/s.txt`, the path of a `x` file, for the same entities; distinguish their path rules"]
    );
    // Different dimension names hide the overlap from the rules alone; the
    // bound paths still show it.
    let text = "source x [s]\npath x: in/{s}.txt\nsource z [t]\npath z: in/{t}.txt/out.txt\noperation f(a, b) -> Text\ncommand f: cp {a} {b} {output}\npath: o/{product}/{entities}\ny = f(x, z @ where(t=1))\nsources:\n    x[s=1]\n    z[t=1]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let error =
        render_bash(&pipeline, &resolve(&pipeline, &inventory.unwrap()).unwrap()).unwrap_err();
    assert!(
        error
            .message()
            .contains("puts it inside `in/1.txt`, the path of `x[s=1]`, which is a file"),
        "{error}"
    );
}

#[test]
fn a_path_rule_that_cannot_name_a_file_says_why() {
    for (rule, reason) in [
        (
            "/tmp/{s}.txt",
            "must be relative to `SPIT_ROOT`, not start with `/`",
        ),
        ("out/{s}/", "must name a file, not end with `/`"),
        (
            "out//{s}",
            "must not contain an empty directory name, as in `//`",
        ),
        ("out/../{s}", "must not contain `.` or `..` directories"),
    ] {
        let text = format!("source x [s]\npath x: {rule}\n");
        let messages = rendered(&diagnose(&text, None), &text);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains(&format!("path rule for `x` {reason}")),
            "{messages:?}"
        );
    }
}

#[test]
fn paths_that_differ_only_in_case_are_flagged() {
    let text = "source x [s]\npath x: in/{s}.txt\noperation f(a) -> Text\npath y: out/{s}.txt\ny = f(x)\nsources:\n    x[s=A]\n    x[s=a]\n";
    assert_eq!(
        rendered(&diagnose(text, None), text),
        [
            "warning: line 2, column 9: `x[s=A]` and `x[s=a]` have paths `in/A.txt` and `in/a.txt`, which differ only in case, so they are one file where case is ignored, as on macOS and Windows",
            "warning: line 4, column 9: `y[s=A]` and `y[s=a]` have paths `out/A.txt` and `out/a.txt`, which differ only in case, so they are one file where case is ignored, as on macOS and Windows",
        ]
    );
}

#[test]
fn bash_refuses_an_empty_inventory() {
    let output = spit(
        &["bash", "examples/commands/bash_demo.spit", "--sources", "-"],
        Some(b"sources:\n"),
    );
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("the inventory lists no source artifacts"),
        "{}",
        text(&output.stderr)
    );
}

#[test]
fn a_root_starting_with_a_dash_is_not_read_as_an_option() {
    let output = spit(
        &[
            "bash",
            "examples/commands/bash_demo.spit",
            "--sources",
            "examples/commands/bash_demo.sources",
        ],
        None,
    );
    assert!(text(&output.stdout)
        .contains("case $SPIT_ROOT in -*) SPIT_ROOT=\"./$SPIT_ROOT\" ;; esac\n"));
}

#[test]
fn single_quotes_and_backslashes_keep_braces_literal() {
    let text = "source x [s]\npath: {product}/{entities}\noperation f(a) -> Text\ncommand f: awk '{print $1}' \\{a\\} \"{a}\" {output}\ny = f(x)\nsources:\n    x[s=1]\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    let script = render_bash(&pipeline, &resolve(&pipeline, &inventory.unwrap()).unwrap()).unwrap();
    assert!(
        script.contains("'awk' '{print $1}' '{a}' \"$SPIT_ROOT\"/'x/s=1' \"$SPIT_ROOT\"/'y/s=1'"),
        "{script}"
    );
}

#[test]
fn shell_operators_in_a_command_are_flagged() {
    let text = "source x [s]\npath: {product}/{entities}\noperation f(a) -> Text\ncommand f: tool {a} {output} 2>&1 | tee '>' log\ny = f(x)\n";
    let messages = rendered(&diagnose(text, None), text);
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].starts_with("warning: line 4, column 30: `2>&1` in the command for `f` is passed to the program as an argument"));
    assert!(messages[1].starts_with("warning: line 4, column 35: `|` in the command"));
}

#[test]
fn a_step_after_an_inline_inventory_is_read_as_a_step() {
    let text = "source x [s]\noperation f(a) -> Text\nsources:\n    x[s=1]\ny = f(x)\naverage : Text [s] = f(x)\n";
    let (pipeline, inventory) = parse_document(text).unwrap();
    assert_eq!(pipeline.invocations.len(), 2);
    assert_eq!(inventory.unwrap().artifacts.len(), 1);
}

#[test]
fn a_bare_lowercase_input_names_an_untyped_port() {
    let pipeline = parse_pipeline("operation f(image, many frames) -> Text @ drop(run)\n").unwrap();
    let ports: Vec<_> = pipeline.operations[0]
        .inputs
        .iter()
        .map(|port| (port.name.as_str(), port.cardinality))
        .collect();
    assert_eq!(
        ports,
        [("image", Cardinality::One), ("frames", Cardinality::Many)]
    );
    let error = parse_pipeline("operation f(a: image) -> Text\n").unwrap_err();
    assert!(error
        .to_string()
        .contains("type `image` must start with a capital letter"));
}

#[test]
fn duplicate_input_ports_are_named() {
    let text = "operation f(a: T, a: T) -> Text\n";
    assert_eq!(
        rendered(&diagnose(text, None), text),
        ["error: line 1, column 11: operation `f` has more than one port named `a`"]
    );
}

#[test]
fn a_byte_order_mark_is_ignored() {
    let tree = Tree::new("bom", &[]);
    let pipeline = tree.path().join("pipeline.spit");
    fs::write(&pipeline, "\u{feff}source x [s]\n").unwrap();
    let output = spit(&["check", pipeline.to_str().unwrap()], None);
    assert!(output.status.success(), "{}", text(&output.stderr));
    fs::write(tree.path().join("lib.spit"), "\u{feff}source y [s]\n").unwrap();
    let output = spit(
        &["check", pipeline.to_str().unwrap(), "--stdin"],
        Some(b"use lib.spit\n"),
    );
    assert!(output.status.success(), "{}", text(&output.stderr));
}

#[cfg(unix)]
#[test]
fn an_import_must_be_a_regular_file() {
    let output = spit(&["check", "x.spit", "--stdin"], Some(b"use /dev/zero\n"));
    assert!(text(&output.stderr).contains("import `/dev/zero` is not a regular file"));
}

#[test]
fn json_mode_always_prints_json() {
    let output = spit(&["check", "x.spit", "--stdin", "--json"], Some(b"\xff"));
    assert!(output.status.success());
    assert_eq!(
        text(&output.stdout),
        "{\"diagnostics\":[{\"severity\":\"error\",\"source\":\"pipeline\",\"line\":null,\"column\":null,\"end_column\":null,\"message\":\"cannot read standard input: stream did not contain valid UTF-8\"}]}\n"
    );
}

#[test]
fn command_line_mistakes_are_named() {
    for (args, problem) in [
        (&["chek", "p.spit"][..], "unknown command `chek`"),
        (&["check"][..], "check needs a pipeline file"),
        (
            &["check", "--json", "p.spit"][..],
            "the pipeline file comes before options such as `--json`",
        ),
        (&["check", "p.spit", "--jsn"][..], "unknown option `--jsn`"),
        (
            &["check", "p.spit", "q.spit"][..],
            "unexpected argument `q.spit`; give one pipeline file",
        ),
        (
            &["check", "p.spit", "--stdin", "--stdin"][..],
            "--stdin is given more than once",
        ),
        (
            &["check", "p.spit", "--sources"][..],
            "--sources needs a value: <inventory.spit|->",
        ),
    ] {
        let output = spit(args, None);
        assert!(!output.status.success());
        let stderr = text(&output.stderr);
        assert!(
            stderr.starts_with(&format!("error: {problem}\nusage: spit ")),
            "{stderr}"
        );
    }
    let output = spit(&["check", "/no/such/pipeline.spit"], None);
    assert!(text(&output.stderr).starts_with("error: cannot read `/no/such/pipeline.spit`: "));
}
