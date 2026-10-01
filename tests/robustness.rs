//! Hostile and unusual input: odd file names under a root, paths that
//! cannot coexist, commands written as if for a shell, and the command line
//! used wrongly.

mod support;

use support::{text, Tree};

use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use spit::{diagnose, parse_pipeline, resolve, Cardinality, Diagnostic};

/// Step 3's binding, with its error as text.
fn bind(pipeline: &spit::Pipeline, dag: &spit::ResolvedDag) -> Result<spit::BoundDag, String> {
    spit::bind_dag(pipeline, dag).map_err(|error| error.to_string())
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

fn rendered(diagnostics: &[Diagnostic], text: &str) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.display_in(text, None).to_string())
        .collect()
}

#[test]
fn a_path_inside_another_artifacts_file_is_rejected() {
    let text = "source x [s]\npath x: in/{s}.txt\noperation f(a) -> Text\ncommand f: cp {a} {output}\npath y: in/{s}.txt/out.txt\ny = f(x)\n";
    assert_eq!(
        rendered(&diagnose(text, None), text),
        ["error: line 5, column 9: path rule for `y` puts files inside `in/s.txt`, the path of a `x` file, for the same entities; distinguish their path rules"]
    );
    // Different dimension names hide the overlap until paths are bound.
    let text = "source x [s]\npath x: in/{s}.txt\nsource z [t]\npath z: in/{t}.txt/out.txt\noperation f(a, b) -> Text\ncommand f: cp {a} {b} {output}\npath: o/{product}/{entities}\ny = f(x, z @ where(t=1))\nsources:\n    x[s=1]\n    z[t=1]\n";
    let (pipeline, inventory) = support::parse_fixture(text).unwrap();
    let error = bind(&pipeline, &resolve(&pipeline, &inventory.unwrap()).unwrap()).unwrap_err();
    assert!(
        error.contains("puts it inside `in/1.txt`, the path of `x[s=1]`, which is a file"),
        "{error}"
    );
}

#[test]
fn a_path_rule_that_cannot_name_a_file_says_why() {
    for (rule, reason) in [
        (
            "/tmp/{s}.txt",
            "must be relative to the dataset root, not start with `/`",
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
    let text =
        "source x [s]\npath x: in/{s}.txt\noperation f(a) -> Text\npath y: out/{s}.txt\ny = f(x)\n";
    assert_eq!(
        rendered(&diagnose(text, Some("sources:\n    x[s=A]\n    x[s=a]\n")), text),
        [
            "warning: line 2, column 9: `x[s=A]` and `x[s=a]` have paths `in/A.txt` and `in/a.txt`, which differ only in case, so they are one file where case is ignored, as on macOS and Windows",
            "warning: line 4, column 9: `y[s=A]` and `y[s=a]` have paths `out/A.txt` and `out/a.txt`, which differ only in case, so they are one file where case is ignored, as on macOS and Windows",
        ]
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
fn a_bare_lowercase_input_names_an_untyped_port() {
    let pipeline = parse_pipeline("operation f(image, frames: many) -> Text\n").unwrap();
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
    assert!(error
        .to_string()
        .contains("pass the lowercase product in the operation call"));
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
    for (args, problem, more) in [
        (
            &["chek", "p.spit"][..],
            "unknown command `chek`",
            "run `spit help`",
        ),
        (
            &["check"][..],
            "check needs <pipeline.spit | recipe.spitin>",
            "usage: spit check ",
        ),
        (
            &["check", "p.spit", "--jsn"][..],
            "unknown option `--jsn`",
            "usage: spit check ",
        ),
        (
            &["check", "p.spit", "q.spit"][..],
            "unexpected file `q.spit`",
            "usage: spit check ",
        ),
        (
            &["check", "p.spit", "--stdin", "--stdin"][..],
            "--stdin is given more than once",
            "usage: spit check ",
        ),
        (
            &["dag", "p.spit", "d.spitout", "--root"][..],
            "--root needs a value: <directory>",
            "usage: spit dag ",
        ),
        (
            &["check", "p.spit", "--frobnicate"][..],
            "unknown option `--frobnicate`",
            "usage: spit check ",
        ),
    ] {
        let output = spit(args, None);
        assert!(!output.status.success());
        let stderr = text(&output.stderr);
        assert!(
            stderr.starts_with(&format!("error: {problem}\n{more}")),
            "{stderr}"
        );
    }
    let output = spit(&["check", "/no/such/pipeline.spit"], None);
    assert!(text(&output.stderr).starts_with("error: cannot read `/no/such/pipeline.spit`: "));
}

#[test]
fn a_byte_order_mark_is_ignored_by_every_entry_point() {
    let bom = |text: &str| format!("\u{feff}{text}");
    let valid = "source raw : Raw [id]\noperation clean(raw: Raw) -> Clean\ncleaned = clean(raw)\n";
    let records = "sources:\n  raw[id=a]\n";
    parse_pipeline(&bom(valid)).unwrap();
    spit::parse_source_inventory(&bom(records)).unwrap();
    spit::parse_input_spec(&bom("pipeline analysis.spit\npath raw: in/{id}.txt\n")).unwrap();
    assert_eq!(
        diagnose(&bom(valid), Some(&bom(records))),
        diagnose(valid, Some(records))
    );
    // An error on the first line has the same columns, counted without it.
    let broken = "source bad [id id]\n";
    let found = diagnose(&bom(broken), None);
    assert_eq!(found, diagnose(broken, None));
    assert!(found[0].is_error());
    assert_eq!(
        spit::render_diagnostics_json(&found, &bom(broken), None),
        spit::render_diagnostics_json(&found, broken, None)
    );
    assert_eq!(
        found[0].display_in(&bom(broken), None).to_string(),
        found[0].display_in(broken, None).to_string()
    );
}

#[test]
fn deeply_nested_type_arguments_are_an_error_not_a_crash() {
    let nested = |depth: usize| {
        format!(
            "source raw : {}B{} [id]\n",
            "A<".repeat(depth),
            ">".repeat(depth)
        )
    };
    assert!(diagnose(&nested(64), None).is_empty());
    for depth in [65, 100_000] {
        let found = diagnose(&nested(depth), None);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].message,
            "type arguments nest more than 64 levels deep"
        );
    }
}
