//! `check:` default lists: the checks every output of a file or stage runs.

mod support;

use spit::diagnose;
use support::{spit, text, Tree};

const DECLARED: &str = "\
check nonempty: test -s {@path}
check lines(n): check_lines {@path} {n}
check ndim(n): check_ndim {@path} {n}
source raw : Text [id]
path raw: in/{id}.txt
operation clean(text: Text @ check(lines(5))) -> Text .txt
command clean: sort -o {@output} {text}
operation split(text: Text) -> (a: Text .txt, b: Text .txt @ check(!nonempty, lines(9)))
command split: sp {text} {a} {b}
";

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| format!("{}: {}", diagnostic.line.unwrap_or(0), diagnostic.message))
        .collect()
}

/// One raw file, and `pipeline` beside it; `spit dag --commands` on it.
fn commands(pipeline: &str) -> String {
    let tree = Tree::new("default_checks", &["data/in/1.txt"]);
    let file = tree.write("p.spit", pipeline);
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

/// The checks that run on `product`'s artifact after the command that
/// writes it: the `check:` lines after its job's `run:` line.
fn after(commands: &str, product: &str) -> Vec<String> {
    let artifact = format!("out/{product}/");
    let mut found = Vec::new();
    for job in commands.split("\n\n") {
        let mut lines = job
            .lines()
            .skip_while(|line| !line.trim_start().starts_with("run:"));
        if !lines.next().is_some_and(|run| run.contains(&artifact)) {
            continue;
        }
        found.extend(
            lines
                .filter(|line| line.contains(&artifact))
                .map(|line| line.trim_start()["check:".len()..].trim().to_owned()),
        );
    }
    found
}

#[test]
fn a_file_default_checks_every_output_but_no_input_or_source() {
    let commands = commands(&format!(
        "{DECLARED}check: nonempty\nfirst = clean(raw)\nsecond = clean(first)\n"
    ));
    assert_eq!(after(&commands, "first"), ["test -s out/first/id=1.txt"]);
    // `second` reads `first` through a port with its own check, which the
    // default does not add; the source is never checked.
    assert_eq!(after(&commands, "second"), ["test -s out/second/id=1.txt"]);
    assert!(commands.contains("check_lines out/first/id=1.txt 5"));
    assert!(!commands.contains("test -s in/1.txt"));
}

#[test]
fn a_stage_default_follows_the_stage_of_the_call() {
    let commands = commands(&format!(
        "{DECLARED}first = clean(raw)\nstage pre:\n    check: ndim(3)\n    second = clean(first)\nthird = clean(second)\n"
    ));
    assert!(after(&commands, "first").is_empty());
    assert_eq!(
        after(&commands, "second"),
        ["check_ndim out/second/id=1.txt 3"]
    );
    assert!(after(&commands, "third").is_empty());
}

#[test]
fn stage_defaults_add_to_the_file_and_the_stages_around_them_in_order() {
    let commands = commands(&format!(
        "{DECLARED}check: nonempty\nstage pre:\n    check: ndim(3)\n    stage deep:\n        check: lines(1)\n        second = clean(raw)\n"
    ));
    assert_eq!(
        after(&commands, "second"),
        [
            "test -s out/second/id=1.txt",
            "check_ndim out/second/id=1.txt 3",
            "check_lines out/second/id=1.txt 1",
        ]
    );
}

#[test]
fn a_stage_or_an_output_opts_out_with_a_bang() {
    let commands = commands(&format!(
        "{DECLARED}check: nonempty\nstage pre:\n    check: ndim(3), !nonempty\n    a, b = split(raw)\nstage plain:\n    c = clean(raw)\n"
    ));
    // The stage drops the file's check; `b` also lists its own.
    assert_eq!(after(&commands, "a"), ["check_ndim out/a/id=1.txt 3"]);
    assert_eq!(
        after(&commands, "b"),
        [
            "check_ndim out/b/id=1.txt 3",
            "check_lines out/b/id=1.txt 9"
        ]
    );
    assert_eq!(after(&commands, "c"), ["test -s out/c/id=1.txt"]);
}

#[test]
fn an_output_opts_out_of_a_file_default_for_itself_alone() {
    let commands = commands(&format!("{DECLARED}check: nonempty\na, b = split(raw)\n"));
    assert_eq!(after(&commands, "a"), ["test -s out/a/id=1.txt"]);
    assert_eq!(after(&commands, "b"), ["check_lines out/b/id=1.txt 9"]);
}

#[test]
fn a_check_on_one_artifact_runs_once_however_many_lists_name_it() {
    let commands = commands(&format!(
        "{DECLARED}operation twice(text: Text) -> Text .txt @ check(nonempty)\ncommand twice: sort -o {{@output}} {{text}}\ncheck: nonempty, nonempty\nstage pre:\n    check: nonempty\n    done = twice(raw)\n"
    ));
    assert_eq!(after(&commands, "done"), ["test -s out/done/id=1.txt"]);
}

#[test]
fn a_producer_default_covers_the_check_a_reader_would_repeat() {
    let commands = commands(&format!(
        "{DECLARED}operation read(text: Text @ check(nonempty)) -> Text .txt\ncommand read: sort -o {{@output}} {{text}}\ncheck: nonempty\nfirst = clean(raw)\nsecond = read(first)\n"
    ));
    assert_eq!(
        commands.matches("test -s out/first/id=1.txt").count(),
        1,
        "{commands}"
    );
}

#[test]
fn defaults_are_checked_like_other_uses() {
    let unknown = errors(&format!("{DECLARED}check: missing\nfirst = clean(raw)\n"));
    assert!(
        unknown
            .iter()
            .any(|e| e.contains("unknown check `missing`")),
        "{unknown:?}"
    );
    let arity = errors(&format!(
        "{DECLARED}stage s:\n    check: ndim\n    first = clean(raw)\n"
    ));
    assert!(
        arity.iter().any(|e| e.contains("takes 1 argument")),
        "{arity:?}"
    );
    let opt_out = errors(&format!("{DECLARED}check: !missing\nfirst = clean(raw)\n"));
    assert!(
        opt_out
            .iter()
            .any(|e| e.contains("unknown check `missing`")),
        "{opt_out:?}"
    );
    let twice = errors(&format!(
        "{DECLARED}check: nonempty\ncheck: nonempty\nfirst = clean(raw)\n"
    ));
    assert!(
        twice.iter().any(|e| e.contains("duplicate `check:` list")),
        "{twice:?}"
    );
    let empty = errors(&format!("{DECLARED}check:\nfirst = clean(raw)\n"));
    assert!(!empty.is_empty());
}

#[test]
fn opting_out_belongs_on_an_output() {
    let input = errors("operation f(x: Text @ check(!nonempty)) -> Text\n");
    assert!(
        input.iter().any(|e| e.contains("cannot opt out")),
        "{input:?}"
    );
}

#[test]
fn a_product_may_still_be_named_check() {
    for step in [
        "check = clean(raw)",
        "check: Text [id] = clean(raw)",
        "check : Text [id] = clean(raw)",
    ] {
        let source = format!("{DECLARED}check: nonempty\n{step}\nmore = clean(check)\n");
        assert!(errors(&source).is_empty(), "{step}: {:?}", errors(&source));
    }
}
