//! Warnings: unused definitions, missing commands, steps that resolve no
//! jobs, and an inline inventory a separate one replaces.

use spit::{diagnose, Diagnostic, Severity};

fn rendered(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics.iter().map(ToString::to_string).collect()
}

fn errors(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .filter(Diagnostic::is_error)
        .collect()
}

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
operation clean(Table) -> Table
operation tag(Table) -> Tagged<$Key>
operation unused(Table) -> Table
command clean: tool {input} {output}
cleaned = clean(raw)
tagged : Tagged<Label> [id] = tag(cleaned)
";
    let diagnostics = diagnose(text, None);
    assert_eq!(
        rendered(&diagnostics),
        [
            "warning: line 2: source product `spare` is never used as an input",
            "warning: line 4: operation `tag` has no command, so `bash` cannot run its jobs",
            "warning: line 4: output type variable `Key` of `tag` appears in no input; it is known only where the output product declares its type",
            "warning: line 5: operation `unused` is declared but never used",
        ]
    );
    assert!(errors(diagnostics).is_empty());

    // A pipeline without commands may be meant for its DAG alone.
    let without_commands = text.replace("command clean: tool {input} {output}\n", "");
    assert!(!rendered(&diagnose(&without_commands, None))
        .iter()
        .any(|line| line.contains("has no command")));

    // A file with no steps is a library of definitions, used by importing it.
    let library = "source raw : Table [id]\noperation clean(Table) -> Table\n";
    assert!(diagnose(library, None).is_empty());
}

#[test]
fn a_line_with_an_error_shows_no_warnings() {
    let text = "source raw : Table [id]\nsource spare : Table [id, id]\noperation clean(Table) -> Table\ncleaned = clean(raw)\n";
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
operation f(Image) -> Image
operation g(Image, Image) -> Image
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
fn a_separate_inventory_replaces_a_malformed_inline_one() {
    let text = "source image [subject]\noperation f(Image) -> Image\nout = f(image)\nsources:\n  image[subject=a\n";
    assert!(diagnose(text, None).iter().any(Diagnostic::is_error));
    let issues: Vec<_> = diagnose(text, Some("sources:\n  image[subject=b]\n"))
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        issues,
        ["warning: line 4: this inline inventory is ignored because a separate inventory was supplied"]
    );
}
