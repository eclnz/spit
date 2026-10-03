//! Runtime checks: `check` declarations, `@ check(...)` on ports and
//! sources, and the checks each job carries in the `.spitdag`.

mod support;

use spit::diagnose;
use support::{spit, text, Tree};

const CHECKS: &str = "\
check nonempty: test -s {@path}
check lines(n): check_lines {@path} {n}
";

/// Lines cleaned one by one, then merged: the source, both input ports and
/// both outputs carry checks.
const LINES: &str = "\
source raw : Text [id] @ check(nonempty)
path raw: in/{id}.txt
operation clean(text: Text @ check(lines(1))) -> Text .txt @ check(nonempty)
command clean: sort -o {@output} {text}
operation merge(items: many Text @ check(nonempty)) -> (all: Text .txt @ check(nonempty, lines(2)))
command merge: cat {items} {all}
cleaned = clean(raw)
merged = merge(cleaned @ vary(id))
";

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| format!("{}: {}", diagnostic.line.unwrap_or(0), diagnostic.message))
        .collect()
}

/// Two raw files, and `pipeline` beside them; `spit dag` on it with `args`.
fn dag(pipeline: &str, args: &[&str]) -> String {
    let tree = Tree::new("checks", &["data/in/1.txt", "data/in/2.txt"]);
    let file = tree.write("p.spit", pipeline);
    let root = tree.path().join("data");
    let mut all = vec![
        "dag",
        file.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
    ];
    all.extend(args);
    let output = spit(&all);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

#[test]
fn each_job_runs_its_checks_in_order_around_its_command() {
    let commands = dag(&format!("{CHECKS}{LINES}"), &["--commands"]);
    assert_eq!(
        commands,
        "\
Job 1  clean
  check:  check_lines in/1.txt 1
  check:  test -s in/1.txt
  run:    sort -o out/cleaned/id=1.txt in/1.txt
  check:  test -s out/cleaned/id=1.txt

Job 2  clean
  check:  check_lines in/2.txt 1
  check:  test -s in/2.txt
  run:    sort -o out/cleaned/id=2.txt in/2.txt
  check:  test -s out/cleaned/id=2.txt

Job 3  merge
  run:    cat out/cleaned/id=1.txt out/cleaned/id=2.txt out/merged/global.txt
  check:  test -s out/merged/global.txt
  check:  check_lines out/merged/global.txt 2
"
    );
}

#[test]
fn the_spitdag_names_each_check_its_port_and_its_artifact() {
    let json = dag(&format!("{CHECKS}{LINES}"), &["--json"]);
    assert!(json.starts_with("{\"version\":6,"), "{json}");
    assert!(json.contains("\"executables\":[\"cat\",\"check_lines\",\"sort\",\"test\"]"));
    assert!(json.contains(
        "\"checks\":[{\"when\":\"before\",\"check\":\"lines(1)\",\"port\":\"text\",\"path\":\"in/1.txt\",\
\"command\":[[\"check_lines\"],[{\"path\":\"in/1.txt\"}],[\"1\"]]},"
    ));
    assert!(json.contains(
        "{\"when\":\"after\",\"check\":\"lines(2)\",\"port\":\"all\",\"path\":\"out/merged/global.txt\","
    ));
}

#[test]
fn checks_change_neither_the_jobs_nor_their_fingerprints() {
    let unchecked = "\
source raw : Text [id]
path raw: in/{id}.txt
operation clean(text: Text) -> Text .txt
command clean: sort -o {@output} {text}
operation merge(items: many Text) -> (all: Text .txt)
command merge: cat {items} {all}
cleaned = clean(raw)
merged = merge(cleaned @ vary(id))
";
    let fingerprints = |json: &str| {
        json.match_indices("\"fingerprint\":")
            .map(|(at, _)| json[at..at + 32].to_owned())
            .collect::<Vec<_>>()
    };
    let checked = dag(&format!("{CHECKS}{LINES}"), &["--json"]);
    let unchecked = dag(unchecked, &["--json"]);
    assert!(!unchecked.contains("\"when\""), "{unchecked}");
    assert_eq!(fingerprints(&checked).len(), 3);
    assert_eq!(fingerprints(&checked), fingerprints(&unchecked));
    let without_checks = |json: &str| {
        let mut json = json.to_owned();
        while let Some(start) = json.find(",\"checks\":[{") {
            let end = json[start..].find("}]}").unwrap() + start + 2;
            json.replace_range(start..end, ",\"checks\":[]");
        }
        json.replace("\"check_lines\",", "")
            .replace(",\"test\"", "")
    };
    // The roots are two temporary folders.
    let after_root = |json: &str| json[json.find("\"external_inputs\"").unwrap()..].to_owned();
    assert_eq!(
        after_root(&without_checks(&checked)),
        after_root(&unchecked)
    );
}

#[test]
fn a_reader_runs_a_check_its_producer_skipped() {
    // `clean` checks its output only for lines, so `merge` still checks
    // that each is not empty.
    let pipeline = format!("{CHECKS}{LINES}").replace(
        "-> Text .txt @ check(nonempty)",
        "-> Text .txt @ check(lines(1))",
    );
    let commands = dag(&pipeline, &["--commands"]);
    assert!(commands.contains(
        "Job 3  merge\n  check:  test -s out/cleaned/id=1.txt\n  check:  test -s out/cleaned/id=2.txt\n  run:"
    ));
}

#[test]
fn checks_and_their_uses_are_checked_where_they_are_written() {
    let pipeline = |rest: &str| format!("{CHECKS}source raw : Text [id]\n{rest}");
    let first = |text: &str| errors(text).into_iter().next().unwrap_or_default();
    assert_eq!(
        first(&pipeline("operation f(x: Text @ check(nope)) -> Text\n")),
        "4: unknown check `nope`; declare it with `check nope: ...`"
    );
    assert_eq!(
        first(&pipeline("operation f(x: Text @ check(lines)) -> Text\n")),
        "4: check `lines` takes 1 argument (n), but `lines` gives 0"
    );
    assert_eq!(
        first(&pipeline(
            "operation f(x: Text @ check(nonempty(2))) -> Text\n"
        )),
        "4: check `nonempty` takes no arguments; write `nonempty`"
    );
    assert_eq!(
        first("check bad(n): test -s {n}\n"),
        "1: check `bad` must use `{@path}`, the artifact it checks"
    );
    assert_eq!(
        first("check bad(n): test -s {@path}\n"),
        "1: check `bad` never uses its parameter `{n}`"
    );
    assert_eq!(
        first("check bad: test {@output} {@path}\n"),
        "1: check `bad` uses unknown placeholder `{@output}`; a check reads only `{@path}`, the artifact it checks"
    );
    assert_eq!(
        first(&format!("{CHECKS}check nonempty: test -f {{@path}}\n")),
        "3: duplicate check `nonempty`"
    );
    assert!(first(&pipeline(
        "operation f(x: Text) -> Text\ny : Text [id] @ check(nonempty) = f(raw)\n"
    ))
    .contains("a step's product takes no `@ check(...)`"));
    assert!(first(&pipeline("operation f(x: Text) @ check(nonempty)\n"))
        .contains("`@ check(...)` follows the port it checks"));
    assert!(first(&pipeline("source other [id] @ vary(id)\n"))
        .contains("a source takes only `@ check(...)`"));
    assert!(first(&pipeline(
        "operation f(x: Text @ check(lines(a b))) -> Text\n"
    ))
    .contains("cannot be a check argument"));
}

#[test]
fn a_step_may_still_make_a_product_named_check() {
    assert!(errors("source raw [id]\noperation f(x) -> Y\ncheck = f(raw)\n").is_empty());
}

#[test]
fn imports_bring_the_checks_their_definitions_attach() {
    let tree = Tree::new("check-imports", &["data/in/1.txt"]);
    tree.write(
        "lib.spit",
        &format!("{CHECKS}source raw : Text [id] @ check(nonempty)\npath raw: in/{{id}}.txt\n"),
    );
    let file = tree.write(
        "p.spit",
        "use lib.spit as l\nuse lines from lib.spit\n\
operation keep(x: Text @ check(lines(2), l::nonempty)) -> Text .txt\n\
command keep: cp {x} {@output}\nk = keep(l::raw)\n",
    );
    let root = tree.path().join("data");
    let output = spit(&[
        "dag",
        file.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--commands",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "Job 1  keep\n  check:  check_lines in/1.txt 2\n  check:  test -s in/1.txt\n  run:    cp in/1.txt out/k/id=1.txt\n"
    );
}
