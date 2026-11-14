//! Tools: the `tool`, `uses` and `default uses:` lines that say which tools
//! each operation's jobs depend on, the `.spitenv` a recipe names, and how a
//! `.spitdag` carries both to a runner.

mod support;

use support::{rendered, spit, text, Tree};

use spit::{diagnose, parse_pipeline, ToolSet};

const HEADER: &str = "source raw : Text .txt [id]
path raw: in/{id}
default path: out/{@product}/{@entities}
default entities: {@key}={@value} separated \"_\"
operation clean(x: Text) -> Text .txt
command clean: sort -o {@output} {x}
operation count(x: Text) -> Text .txt
command count: wc -l {x} > {@output}
";

/// The tools of each operation the pipeline `HEADER` and `rest` make declares, by name.
fn tools(rest: &str) -> Vec<(String, ToolSet)> {
    let pipeline = parse_pipeline(&format!("{HEADER}{rest}")).unwrap();
    pipeline
        .operations
        .into_iter()
        .map(|operation| (operation.name, operation.tools))
        .collect()
}

fn only(names: &[&str]) -> ToolSet {
    ToolSet::Only(names.iter().map(|name| (*name).to_owned()).collect())
}

#[test]
fn with_no_uses_every_job_depends_on_every_tool() {
    for (_, set) in tools("c = clean(raw)\n") {
        assert_eq!(set, ToolSet::default());
    }
}

#[test]
fn lists_add_up_from_the_file_through_each_stage_to_the_operation() {
    let found = tools(
        "tool sorter, counter, archiver
default uses: sorter
uses count: counter, !sorter
stage outer:
    default uses: archiver
    stage inner:
        default:
            uses: counter
        operation pack(x: Text) -> Text .txt
        command pack: tar cf {@output} {x}
        p = pack(raw)
c = clean(raw)
n = count(c)
",
    );
    assert_eq!(
        found,
        [
            ("clean".to_owned(), only(&["sorter"])),
            ("count".to_owned(), only(&["counter"])),
            ("pack".to_owned(), only(&["sorter", "archiver", "counter"])),
        ]
    );
}

#[test]
fn a_mistake_in_a_list_is_reported_where_it_is_written() {
    let errors = |rest: &str| rendered(&diagnose(&format!("{HEADER}{rest}"), None));
    let found = errors("tool sorter\nuses count: sortr\nc = clean(raw)\n");
    assert!(
        found[0].contains("line 10") && found[0].contains("tool `sortr` is not declared"),
        "{found:?}"
    );
    let found = errors("tool sorter\nuses missing: sorter\nc = clean(raw)\n");
    assert!(found[0].contains("names operation `missing`"), "{found:?}");
    let found = errors("tool sorter\nuses clean: sorter\nuses clean: sorter\nc = clean(raw)\n");
    assert!(found[0].contains("duplicate `uses` line"), "{found:?}");
    let found = errors("tool sorter\ntool sorter\nc = clean(raw)\n");
    assert!(found[0].contains("already declared"), "{found:?}");
    let found = errors("tools from m.spitenv\nc = clean(raw)\n");
    assert!(
        found[0].contains("belongs in a .spitin recipe"),
        "{found:?}"
    );
    let found = errors("uses: sorter\nc = clean(raw)\n");
    assert!(found[0].contains("default uses: ..."), "{found:?}");
}

/// A dataset with the pipeline `HEADER` and `rest`, a recipe naming
/// `machine.spitenv`, and that file holding `tools`.
fn dataset(rest: &str, tools: &str) -> Tree {
    let tree = Tree::new("tools", &["data/in/1.txt"]);
    tree.write("main.spit", &format!("{HEADER}{rest}"));
    tree.write(
        "main.spitin",
        "pipeline main.spit\nroot data\ntools from machine.spitenv\n",
    );
    tree.write("machine.spitenv", tools);
    tree
}

#[test]
fn the_spitdag_lists_each_tool_and_each_job_names_its_own() {
    let tree = dataset(
        "tool sorter, counter\ndefault uses: sorter\nuses count: counter, !sorter\nc = clean(raw)\nn = count(c)\n",
        "tools:\n    sorter: probe sort --version\n    counter: coreutils 9.4  # as packaged\n",
    );
    let recipe = tree.path().join("main.spitin");
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    let json = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        json.contains("\"executables\":[\"sort\",\"wc\"],\"tools\":[{\"name\":\"sorter\",\"identity\":null,\"probe\":[\"sort\",\"--version\"]},{\"name\":\"counter\",\"identity\":\"coreutils 9.4\",\"probe\":null}],"),
        "{json}"
    );
    assert!(
        json.contains("\"memory\":null,\"tools\":[\"sorter\"],\"anchored\":false}"),
        "{json}"
    );
    assert!(
        json.contains("\"memory\":null,\"tools\":[\"counter\"],\"anchored\":false}"),
        "{json}"
    );
}

#[test]
fn a_recipe_s_spitenv_must_list_every_declared_tool() {
    let tree = dataset(
        "tool sorter, counter\nuses count: counter\nc = clean(raw)\n",
        "tools:\n    sorter: probe sort --version\n",
    );
    let recipe = tree.path().join("main.spitin");
    let output = spit(&["check", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("line 3, column 1: the pipeline declares tool `counter`, which `machine.spitenv` (named on line 3) does not list"),
        "{stderr}"
    );
}

#[test]
fn a_spitenv_is_checked_alone_and_through_its_recipe() {
    let tree = dataset("c = clean(raw)\n", "tools:\n    sorter: probe sort > out\n");
    let alone = spit(&[
        "check",
        tree.path().join("machine.spitenv").to_str().unwrap(),
    ]);
    assert!(!alone.status.success());
    assert!(
        text(&alone.stderr).contains("line 2, column 5: `sorter`'s probe runs with no shell"),
        "{}",
        text(&alone.stderr)
    );
    let recipe = spit(&["check", tree.path().join("main.spitin").to_str().unwrap()]);
    assert!(
        text(&recipe.stderr).contains("line 3, column 1: machine.spitenv line 2: `sorter`'s probe"),
        "{}",
        text(&recipe.stderr)
    );
}

#[test]
fn without_a_spitenv_jobs_depend_on_no_tool() {
    let tree = Tree::new("tools-none", &["data/in/1.txt"]);
    tree.write(
        "main.spit",
        &format!("root data\ntool sorter\nuses clean: sorter\n{HEADER}c = clean(raw)\n"),
    );
    let output = spit(&[
        "dag",
        tree.path().join("main.spit").to_str().unwrap(),
        "--json",
    ]);
    let json = text(&output.stdout);
    assert!(json.contains("\"tools\":[],\"removed\""), "{json}");
    assert!(
        json.contains("\"memory\":null,\"tools\":[],\"anchored\":false}"),
        "{json}"
    );
}

#[test]
fn an_anchored_operation_marks_its_jobs_and_a_wrong_name_is_an_error() {
    let pipeline = parse_pipeline(&format!(
        "{HEADER}anchored clean\nanchored:\n    count\nc = clean(raw)\n"
    ))
    .unwrap();
    assert!(pipeline
        .operations
        .iter()
        .all(|operation| operation.anchored));
    let tree = Tree::new("anchored", &["data/in/1.txt"]);
    tree.write(
        "main.spit",
        &format!("root data\n{HEADER}anchored count\nc = clean(raw)\nn = count(c)\n"),
    );
    let output = spit(&[
        "dag",
        tree.path().join("main.spit").to_str().unwrap(),
        "--json",
    ]);
    let json = text(&output.stdout);
    assert_eq!(json.matches("\"anchored\":true}").count(), 1, "{json}");
    assert_eq!(json.matches("\"anchored\":false}").count(), 1, "{json}");
    let hovers = spit(&[
        "check",
        tree.path().join("main.spit").to_str().unwrap(),
        "--json",
        "--hovers",
    ]);
    assert!(
        text(&hovers.stdout).contains("Anchored: its outputs hold their own paths"),
        "{}",
        text(&hovers.stdout)
    );
    let errors = |rest: &str| rendered(&diagnose(&format!("{HEADER}{rest}"), None));
    let found = errors("anchored clen\nc = clean(raw)\n");
    assert!(
        found[0].contains("`anchored` names operation `clen`, which is not declared"),
        "{found:?}"
    );
    let found = errors("anchored clean\nanchored clean\nc = clean(raw)\n");
    assert!(found[0].contains("already marked `anchored`"), "{found:?}");
}

#[test]
fn uses_and_anchored_may_come_before_the_operation_they_name() {
    let pipeline = parse_pipeline(
        "tool sorter, counter
uses late: counter
anchored late
source raw : Text .txt [id]
path raw: in/{id}
default path: out/{@product}/{@entities}
default entities: {@key}={@value} separated \"_\"
operation late(x: Text) -> Text .txt
command late: sort -o {@output} {x}
l = late(raw)
",
    )
    .unwrap();
    let late = &pipeline.operations[0];
    assert_eq!(late.tools, only(&["counter"]));
    assert!(late.anchored);
}

#[test]
fn a_removal_means_the_same_wherever_it_is_in_a_list() {
    let rest = "tool sorter, counter, archiver\n";
    let first = tools(&format!(
        "{rest}uses clean: !sorter, counter\nc = clean(raw)\n"
    ));
    let last = tools(&format!(
        "{rest}uses clean: counter, !sorter\nc = clean(raw)\n"
    ));
    assert_eq!(first, last);
    assert_eq!(first[0].1, only(&["counter"]));
    let removed = tools(&format!("{rest}uses clean: !sorter\nc = clean(raw)\n"));
    assert!(!removed[0].1.includes("sorter") && removed[0].1.includes("counter"));
}

#[test]
fn each_compile_error_of_tool_uses_and_anchored_names_its_place() {
    // `HEADER` is eight lines, so `rest` starts on line 9.
    for (rest, line, columns, message) in [
        ("tool fsl-6\n", 9, 5..10, "invalid tool name `fsl-6`"),
        (
            "tool sorter\nuses clean: my-tool\n",
            10,
            12..19,
            "invalid tool name `my-tool`",
        ),
        (
            "tool sorter\nuses clean: sorter, sorter\n",
            10,
            20..26,
            "named twice in one `uses` list",
        ),
        (
            "anchored clean, clean\n",
            9,
            16..21,
            "operation `clean` is named twice",
        ),
        (
            "anchored:\n    clen\n",
            10,
            4..8,
            "`anchored` names operation `clen`",
        ),
        (
            "default uses: sortr\n",
            9,
            14..19,
            "tool `sortr` is not declared",
        ),
    ] {
        let found = diagnose(&format!("{HEADER}{rest}c = clean(raw)\n"), None);
        let first = found.first().expect("an error");
        assert!(first.message.contains(message), "{rest}: {found:?}");
        assert_eq!(
            (first.line, first.columns.clone()),
            (Some(line), Some(columns)),
            "{rest}: {found:?}"
        );
    }
    let errors = |rest: &str| rendered(&diagnose(&format!("{HEADER}{rest}"), None));
    for (rest, message) in [
        ("tool sorter\nuses clean: sorter,\n", "separates two tools"),
        ("uses clean:\n", "names one or more tools"),
        ("uses clean sorter\n", "expected `uses operation: tools`"),
        ("anchored clean,\n", "expected `anchored operation`"),
    ] {
        let found = errors(&format!("{rest}c = clean(raw)\n"));
        assert!(
            found.first().is_some_and(|first| first.contains(message)),
            "{rest}: {found:?}"
        );
    }
}

#[test]
fn an_imported_operation_keeps_its_tools_and_mark_and_the_importer_cannot_change_them() {
    let tree = Tree::new("tools-import", &["data/in/1.txt"]);
    tree.write(
        "lib.spit",
        "tool sorter\nuses clean: sorter\nanchored clean\noperation clean(x: Text) -> Text .txt\ncommand clean: sort -o {@output} {x}\n",
    );
    let main = "root data\nuse lib.spit\nsource raw : Text .txt [id]\npath raw: in/{id}\ndefault path: out/{@product}/{@entities}\ndefault entities: {@key}={@value} separated \"_\"\nc = clean(raw)\n";
    tree.write("main.spit", main);
    let output = spit(&[
        "dag",
        tree.path().join("main.spit").to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        text(&output.stdout).contains("\"anchored\":true}"),
        "{}",
        text(&output.stdout)
    );
    for (line, message) in [
        (
            "anchored clean\n",
            "mark it `anchored` in the file that declares it",
        ),
        (
            "uses clean: sorter\n",
            "its tools are set by `uses` in the file that declares it",
        ),
    ] {
        tree.write("main.spit", &format!("{main}{line}"));
        let output = spit(&["check", tree.path().join("main.spit").to_str().unwrap()]);
        assert!(
            text(&output.stderr).contains(message),
            "{}",
            text(&output.stderr)
        );
    }
}
