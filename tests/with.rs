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

/// A pipeline with `with` lines at three scopes, and a recipe beside it.
fn recipe_props(recipe: &str) -> (bool, String) {
    let tree = Tree::new("with_recipe", &["data/in/1.txt"]);
    tree.write(
        "p.spit",
        "with: cpus=1 mem=2G\n\
         source raw : Text [id]\n\
         path raw: in/{id}.txt\n\
         operation copy(input: Text) -> Text .txt\n\
         command copy: cp {input} {@output}\n\
         with operation copy: cpus=4\n\
         copied = copy(raw)\n",
    );
    let file = tree.write("p.spitin", &format!("pipeline p.spit\nroot data\n{recipe}"));
    let output = spit(&["dag", file.to_str().unwrap(), "--json"]);
    let shown = if output.status.success() {
        let json = text(&output.stdout);
        let at = json.find("\"props\":").unwrap() + 8;
        json[at..=at + json[at..].find('}').unwrap()].to_owned()
    } else {
        text(&output.stderr)
    };
    (output.status.success(), shown)
}

#[test]
fn a_recipe_sets_properties_over_the_same_scope_of_the_pipeline() {
    // The file's line is overridden for `mem`; the operation's `cpus` is
    // narrower than the recipe's file line, so it stays.
    let (ok, props) = recipe_props("with: mem=64G queue=big cpus=16\n");
    assert!(ok, "{props}");
    assert_eq!(props, r#"{"cpus":"4","mem":"64G","queue":"big"}"#);
    // The recipe's own operation line replaces the pipeline's, and `-`
    // takes a key away; a product line is narrowest of all.
    let (ok, props) = recipe_props(
        "with operation copy: cpus=- time=2h\nwith product copied: queue=\"long jobs\"\n",
    );
    assert!(ok, "{props}");
    assert_eq!(props, r#"{"mem":"2G","queue":"long jobs","time":"2h"}"#);
}

#[test]
fn a_recipe_with_line_must_name_what_the_pipeline_has() {
    let (ok, message) = recipe_props("with operation nope: cpus=1\n");
    assert!(!ok);
    assert!(
        message.contains("which the pipeline does not declare"),
        "{message}"
    );
    let (ok, message) = recipe_props("with product raw: cpus=1\n");
    assert!(!ok);
    assert!(
        message.contains("no step of the pipeline makes"),
        "{message}"
    );
    let (ok, message) = recipe_props("with: cpus=1\nwith: cpus=2\n");
    assert!(!ok);
    assert!(message.contains("duplicate `with:`"), "{message}");
}
