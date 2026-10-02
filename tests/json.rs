//! `check --json`: the diagnostics as JSON for editors, read from standard input.

use std::io::Write;
use std::process::{Command, Output, Stdio};

mod support;
use support::Tree;

/// The output of `spit check --json --stdin` given `pipeline` as the unsaved text.
fn check_json(pipeline: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", "unsaved.spit", "--json", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(pipeline).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn cli_accepts_stdin_and_returns_json() {
    let output = check_json(b"source raw [id]\nthis is invalid\n");
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"source\":\"pipeline\""));
    assert!(json.contains("\"line\":2"));
    assert!(
        json.contains("\"message\":\"`this` does not start a statement"),
        "{json}"
    );
}

#[test]
fn cli_reports_semantic_error_line_in_json() {
    let output = check_json(b"source raw : A<Native> [id]\noperation first(a: A<X>) -> B<X>\nmiddle = first(raw)\noperation second(b: B<Standard>) -> C\nfinal = second(middle)\n",);
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(json.contains("\"line\":5"), "{json}");
    assert!(json.contains("B<Native>"), "{json}");
}

#[test]
fn cli_json_includes_each_severity_and_its_columns() {
    let output = check_json(
        b"source raw [id]\nsource spare [id]\noperation copy(input)\nresult = copy(rwa)\n",
    );
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"diagnostics\":[\
{\"severity\":\"warning\",\"source\":\"pipeline\",\"line\":1,\"column\":8,\"end_column\":11,\"message\":\"source product `raw` is never used as an input\"},\
{\"severity\":\"warning\",\"source\":\"pipeline\",\"line\":2,\"column\":8,\"end_column\":13,\"message\":\"source product `spare` is never used as an input\"},\
{\"severity\":\"error\",\"source\":\"pipeline\",\"line\":4,\"column\":15,\"end_column\":18,\"message\":\"unknown product `rwa`\"}]}\n"
    );
}

#[test]
fn cli_json_columns_count_utf16_code_units() {
    let output =
        check_json("source raw [id]\noperation copy(input)\nx = copy(résumé)\n".as_bytes());
    let json = String::from_utf8(output.stdout).unwrap();
    // `é` is two bytes but one UTF-16 code unit, so `résumé` spans 10..16.
    assert!(
        json.contains("\"line\":3,\"column\":10,\"end_column\":16"),
        "{json}"
    );
}

#[test]
fn recipe_json_places_pipeline_errors_in_the_pipeline_file() {
    let tree = Tree::new("json-recipe-file", &[]);
    let pipeline = tree.write(
        "analysis.spit",
        "source raw [id]\noperation copy(input)\nresult = copy(rwa)\n",
    );
    let recipe = tree.write("data.spitin", "pipeline analysis.spit\nroot .\n");
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(["check", recipe.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout).unwrap();
    assert!(
        json.contains(&format!("\"file\":\"{}\"", pipeline.display())),
        "{json}"
    );
    assert!(
        json.contains("\"line\":3,\"column\":15,\"end_column\":18"),
        "{json}"
    );
    assert!(json.contains("unknown product `rwa`"), "{json}");
}
