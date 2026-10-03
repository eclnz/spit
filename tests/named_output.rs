//! A single output named as an input is, `-> name: Type`, which means the
//! same as `-> (name: Type)` and is used as `{name}`.

mod support;

use spit::{diagnose, parse_pipeline};
use support::{spit, text, Tree};

/// The first error `diagnose` reports for `pipeline`.
fn first_error(pipeline: &str) -> String {
    diagnose(pipeline, None)
        .into_iter()
        .find(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
        .unwrap_or_default()
}

/// Two raw files, and the pipeline files `files` beside them, the last of
/// which is planned; `spit dag --commands` on it.
fn commands(files: &[(&str, &str)]) -> String {
    let tree = Tree::new("named-output", &["data/in/1.txt", "data/in/2.txt"]);
    let mut file = None;
    for (name, contents) in files {
        file = Some(tree.write(name, contents));
    }
    let file = file.expect("a pipeline file");
    let root = tree.path().join("data");
    let output = spit(&[
        "dag",
        file.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--commands",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

const SOURCE: &str = "source dark : Img [id]\npath dark: in/{id}.txt\n";

#[test]
fn a_bare_named_output_is_the_parenthesized_one() {
    for (bare, parenthesized) in [
        ("cali: Pair", "(cali: Pair)"),
        ("cali: Pair .txt", "(cali: Pair .txt)"),
        ("cali: .txt", "(cali: .txt)"),
        ("cali: Pair /", "(cali: Pair /)"),
        (
            "cali: Pair .txt @ check(nonempty)",
            "(cali: Pair .txt @ check(nonempty))",
        ),
    ] {
        let operation = |outputs: &str| {
            let text = format!(
                "check nonempty: test -s {{@path}}\noperation cal(dark: Img) -> {outputs}\n"
            );
            let pipeline = parse_pipeline(&text).unwrap();
            pipeline.operations[0].outputs.clone()
        };
        assert_eq!(operation(bare), operation(parenthesized), "{bare}");
    }
}

#[test]
fn a_bare_named_output_is_written_and_checked_by_its_name() {
    let pipeline = format!(
        "{SOURCE}check nonempty: test -s {{@path}}\n\
         operation cal(dark: Img) -> cali: Pair .txt @ check(nonempty)\n\
         command cal: calibrate {{dark}} -o {{cali.dir}} -n {{cali.stem}}\n\
         x = cal(dark)\n"
    );
    assert_eq!(
        commands(&[("p.spit", &pipeline)]),
        "\
Job 1  cal
  run:    calibrate in/1.txt -o out/x -n id=1
  check:  test -s out/x/id=1.txt

Job 2  cal
  run:    calibrate in/2.txt -o out/x -n id=2
  check:  test -s out/x/id=2.txt
"
    );
}

#[test]
fn an_imported_operation_keeps_its_bare_named_output() {
    let library = format!(
        "{SOURCE}operation cal(dark: Img) -> cali: Pair .txt\n\
         command cal: calibrate {{dark}} {{cali}}\n"
    );
    let main = "use lib.spit as l\nx = l::cal(l::dark)\n";
    assert_eq!(
        commands(&[("lib.spit", &library), ("p.spit", main)]),
        "\
Job 1  l::cal
  run:    calibrate in/1.txt out/x/id=1.txt

Job 2  l::cal
  run:    calibrate in/2.txt out/x/id=2.txt
"
    );
}

#[test]
fn operation_clauses_still_follow_a_bare_named_output() {
    let error = first_error(
        "source dark : Img [id]\noperation cal(dark: many Img) -> cali: Pair @ min(2)\n",
    );
    assert!(
        error.contains("`cal(dark: many Img @ min(2)) -> cali: Pair`"),
        "{error}"
    );
}

#[test]
fn a_bare_named_output_is_named_as_an_output_in_parentheses_is() {
    let cases = [
        (
            "-> Cali: Pair",
            "`Cali` before `:` names the output, and starts with a capital letter as a type does; name it in lowercase, as in `-> cali: Pair`",
        ),
        ("-> dark: Pair", "more than one port named `dark`"),
        ("-> output: Pair", "the name `output` is reserved"),
        (
            "-> cali .txt",
            "to name the output `cali`, give its type after `:`, as in `-> cali: Type`",
        ),
    ];
    for (outputs, expected) in cases {
        let error = first_error(&format!(
            "source dark : Img [id]\noperation cal(dark: Img) {outputs}\ncommand cal: t {{dark}} {{cali}}\nx = cal(dark)\n"
        ));
        assert!(error.contains(expected), "{outputs}: {error}");
    }
}

#[test]
fn a_command_is_told_which_spelling_its_output_takes() {
    let cases = [
        (
            "-> cali: Pair",
            "{@output}",
            "`cal`'s output is named, so write `{cali}`",
        ),
        (
            "-> cali: Pair .txt",
            "{@output.stem}",
            "`cal`'s output is named, so write `{cali.stem}`",
        ),
        (
            "-> (cali: Pair, meta: Json)",
            "{@output} {meta}",
            "`cal` names its outputs, so write one of `{cali}`, `{meta}`",
        ),
        (
            "-> Pair",
            "{cali}",
            "`cal`'s output is unnamed, so write `{@output}`, or name it, as in `-> cali: Pair`",
        ),
    ];
    for (outputs, placeholder, expected) in cases {
        let error = first_error(&format!(
            "source dark : Img [id]\noperation cal(dark: Img) {outputs}\ncommand cal: t {{dark}} {placeholder}\n"
        ));
        assert!(error.contains(expected), "{outputs}: {error}");
    }
}
