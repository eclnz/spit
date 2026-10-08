//! A multi-byte character in a written type is an error with a place, never
//! a panic. The type parser's span must be a valid range of the type's text.

mod support;

use support::errors;
use support::text;

use spit::diagnose;

/// Each place a type is written, with `{}` where the damaged text goes.
const PLACES: [&str; 5] = [
    "source raw : {} [id]\n",
    "operation copy(mri: {}) -> MRI<B0>\n",
    "operation copy(mri: MRI<B0>) -> {}\n",
    "source raw : MRI<B0> [id]\noperation copy(mri: MRI<B0>) -> MRI<B0>\nout : {} = copy(raw)\n",
    "operation copy(mri: MRI<B0>) -> MRI<B0>\nstep : {} = copy(raw)\n",
];

const TYPES: [&str; 7] = [
    "MRI<B0éSeries,S>",
    "MRI<B0,S\u{feff}>",
    "MRI<日>",
    "MRI<😀,S>",
    "é",
    "MRI<B0é",
    "MRI<$é>",
];

#[test]
fn a_multi_byte_character_in_a_type_is_an_error_with_a_place() {
    for place in PLACES {
        for ty in TYPES {
            let text = place.replace("{}", ty);
            let issues = errors(diagnose(&text, None));
            assert!(!issues.is_empty(), "no error for {text:?}");
            assert!(
                issues.iter().all(|issue| issue.line.is_some()),
                "an error without a line for {text:?}"
            );
        }
    }
}

#[test]
fn a_non_ascii_character_inside_angle_brackets_is_reported_at_it() {
    let text = "operation mean_b0(mri: MRI<B0éSeries,S>) -> MRI<B0,S>\n";
    let issues = errors(diagnose(text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(1));
}

#[test]
fn check_json_reports_the_same_location_and_fails() {
    let source = "operation mean_b0(mri: MRI<B0éSeries,S>) -> MRI<B0,S>\n";
    let run = |json: bool| {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut command = Command::new(env!("CARGO_BIN_EXE_spit"));
        command.args(["check", "broken.spit", "--stdin"]);
        if json {
            command.arg("--json");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let plain = run(false);
    let json = run(true);
    assert_eq!(plain.status.code(), Some(1));
    assert_eq!(json.status.code(), Some(1));
    let shown = text(&plain.stderr);
    let structured = text(&json.stdout);
    assert!(shown.contains("line 1, column 30"), "{shown}");
    assert!(
        structured.contains("\"line\":1,\"column\":30"),
        "{structured}"
    );
    assert!(json.stderr.is_empty());
}
