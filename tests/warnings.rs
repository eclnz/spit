//! Warnings: unused definitions, missing commands, steps that resolve no
//! jobs, and an inline inventory a separate one replaces.

mod support;

use support::{errors, rendered};

use spit::{diagnose, Diagnostic, Severity};

fn warnings(diagnostics: Vec<Diagnostic>) -> Vec<String> {
    diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .map(|diagnostic| diagnostic.to_string())
        .collect()
}

#[test]
fn warnings_flag_unused_definitions_missing_commands_and_unbound_type_variables() {
    let text = "\
source raw : Table [id]
source spare : Table [id]
operation clean(table: Table) -> Table
operation tag(table: Table) -> Tagged<$Key>
operation unused(table: Table) -> Table
command clean: tool {table} {output}
cleaned = clean(raw)
tagged : Tagged<Label> [id] = tag(cleaned)
";
    let diagnostics = diagnose(text, None);
    assert_eq!(
        rendered(&diagnostics),
        [
            "warning: line 2: source product `spare` is never used as an input",
            "warning: line 4: operation `tag` has no command, so its jobs cannot run",
            "warning: line 4: output type variable `Key` of `tag` appears in no input; it is known only where the output product declares its type",
            "warning: line 5: operation `unused` is declared but never used",
        ]
    );
    assert!(errors(diagnostics).is_empty());

    // A pipeline without commands may be meant for its DAG alone.
    let without_commands = text.replace("command clean: tool {table} {output}\n", "");
    assert!(!rendered(&diagnose(&without_commands, None))
        .iter()
        .any(|line| line.contains("has no command")));

    // A file with no steps is a library of definitions, used by importing it.
    let library = "source raw : Table [id]\noperation clean(table: Table) -> Table\n";
    assert!(diagnose(library, None).is_empty());
}

#[test]
fn a_line_with_an_error_shows_no_warnings() {
    let text = "source raw : Table [id]\nsource spare : Table [id, id]\noperation clean(table: Table) -> Table\ncleaned = clean(raw)\n";
    assert_eq!(
        rendered(&diagnose(text, None)),
        ["error: line 2: product `spare` has duplicate or empty dimensions"]
    );
}

#[test]
fn steps_that_resolve_no_jobs_are_reported() {
    let text = "\
source image [subject]
source extra [subject]
operation f(image: Image) -> Image
operation g(image: Image, image2: Image) -> Image
cleaned = f(image)
other = f(extra @ where(subject=z))
both = g(cleaned, image)
";
    assert_eq!(
        warnings(diagnose(text, Some("sources:\n  extra[subject=a]\n"))),
        [
            "warning: line 1: source `image` has no artifacts in the inventory, so these steps resolve no jobs: cleaned, both",
            "warning: line 6: `other` resolves no jobs: its inputs have artifacts, but none match each other or the step's selectors",
        ]
    );
    // Without an inventory, no step is expected to resolve jobs.
    assert!(warnings(diagnose(text, None)).is_empty());
}

#[test]
fn steps_that_read_a_product_twice_are_checked_in_linear_time() {
    // Each step reads the one before twice. Following every path from a
    // step back to its sources would take 2^40 visits here.
    let mut text = String::from(
        "source s0 : T [id]\nsource other : T [id]\noperation g(t: T, t2: T) -> T\n\
         path: out/{@product}/{id}.txt\npath s0: in/{id}.txt\npath other: o/{id}.txt\n",
    );
    for step in 1..=40 {
        let input = if step == 1 {
            "s0".to_owned()
        } else {
            format!("p{}", step - 1)
        };
        text += &format!("p{step} : T [id] = g({input}, {input})\n");
    }
    let found = warnings(diagnose(&text, Some("sources:\n  other[id=a]\n")));
    assert!(
        found[0].contains("source `s0` has no artifacts in the inventory")
            && found[0].ends_with("p39, p40"),
        "{found:?}"
    );
}

#[test]
fn a_product_named_after_its_operation_is_warned_about() {
    let text = "\
source log : Log [day]
operation digest(log: Log) -> Digest
command digest: logdigest {log} {output}
digest = digest(log)
";
    let diagnostics = diagnose(text, None);
    assert_eq!(
        rendered(&diagnostics),
        ["warning: line 4: product `digest` has the name of the operation that makes it; name the result instead, so the step reads as what it makes"]
    );
    let renamed = text.replace("digest = digest(log)", "daily = digest(log)");
    assert!(warnings(diagnose(&renamed, None)).is_empty());
}
