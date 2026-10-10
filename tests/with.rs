//! `with` lines: the properties each job carries in the `.spitdag`, merged
//! from the narrowest scope that names a key.

mod support;

use spit::diagnose;
use support::{spit, text, Tree};

const PIPELINE: &str = "\
with: cpus=1 mem=2G
source raw : Text [id]
path raw: in/{id}.txt

stage pre:
    with: mem=8G
    operation sort_lines(input: Text) -> Text .txt
    command sort_lines: sort -o {@output} {input}
    with operation sort_lines: cpus=8 time=6h
    sorted = sort_lines(raw)
    with product sorted: mem=-

    stage inner:
        again = sort_lines(sorted)

operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
copied = copy(raw)
";

/// The `props` of each job of `pipeline`, in job order.
fn props(pipeline: &str) -> Vec<String> {
    let tree = Tree::new("with", &["data/in/1.txt"]);
    let file = tree.write("p.spit", pipeline);
    let root = tree.path().join("data");
    let output = spit(&[
        "dag",
        file.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let json = text(&output.stdout);
    json.match_indices("\"props\":")
        .map(|(at, _)| {
            let rest = &json[at + 8..];
            rest[..=rest.find('}').unwrap()].to_owned()
        })
        .collect()
}

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| format!("{}: {}", diagnostic.line.unwrap_or(0), diagnostic.message))
        .collect()
}

#[test]
fn the_narrower_scope_wins_key_by_key() {
    assert_eq!(
        props(PIPELINE),
        [
            // `sorted`: the product takes `mem` away, the operation sets
            // `cpus` and `time`.
            r#"{"cpus":"8","time":"6h"}"#,
            // `again`: the inner stage inherits its stage's `mem`.
            r#"{"cpus":"8","mem":"8G","time":"6h"}"#,
            // `copied`: outside every stage, only the file's line reaches it.
            r#"{"cpus":"1","mem":"2G"}"#,
        ]
    );
}

#[test]
fn a_pipeline_without_with_gives_jobs_empty_props() {
    let plain = "\
source raw : Text [id]
path raw: in/{id}.txt
operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
copied = copy(raw)
";
    assert_eq!(props(plain), ["{}"]);
}

#[test]
fn a_step_may_still_make_a_product_named_with() {
    let step = "\
source raw : Text [id]
path raw: in/{id}.txt
operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
with = copy(raw)
";
    assert_eq!(props(step), ["{}"]);
}

#[test]
fn mistakes_in_a_with_line_are_errors_on_its_line() {
    let base = "\
source raw : Text [id]
operation copy(input: Text) -> Text .txt
command copy: cp {input} {@output}
copied = copy(raw)
";
    let line = |extra: &str| errors(&format!("{base}{extra}\n"));
    assert!(line("with: cpus=1\nwith: mem=1G")[0].starts_with("6: duplicate `with:`"));
    assert!(line("with operation nope: cpus=1")[0].contains("not declared above"));
    assert!(line("with product raw: cpus=1")[0].contains("no step makes"));
    assert!(
        line("with product copied: cpus=1\nwith product copied: cpus=2")[0]
            .starts_with("6: duplicate `with product copied`")
    );
    assert!(line("with: Cpus=1")[0].contains("not a property name"));
    assert!(line("with: cpus=1 cpus=2")[0].contains("given twice"));
    assert!(line("with: cpus=")[0].contains("has no value"));
}

#[test]
fn an_operation_with_a_body_takes_no_with() {
    let text = "\
source raw : Text [id]
operation inner(input: Text) -> Text .txt
command inner: cp {input} {@output}
operation outer(input: Text) -> (result: Text):
    result = inner(input)
with operation outer: cpus=4
copied = outer(raw)
";
    let found = errors(text);
    assert!(
        found
            .iter()
            .any(|e| e.contains("carried out by the steps in its body")),
        "{found:?}"
    );
}

#[test]
fn a_recipe_cannot_hold_a_with_line() {
    let tree = Tree::new("with_recipe", &["data/in/1.txt"]);
    tree.write("p.spit", "source raw : Text [id]\npath raw: in/{id}.txt\n");
    let recipe = tree.write("p.spitin", "pipeline p.spit\nroot data\nwith: cpus=2\n");
    let output = spit(&["dag", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(text(&output.stderr).contains("belongs in the .spit pipeline"));
}
