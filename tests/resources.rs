//! What each job of an operation needs, from its `resources` line: how a
//! job carries it to a backend, `{@cores}` in its command, and what a
//! malformed line reports.

mod support;

use support::rendered;

use spit::{bind_dag, diagnose, parse_pipeline, parse_source_inventory, resolve};

const HEADER: &str = "source table : Table .tsv [sheet]
path table: in/{sheet}
default path: out/{@product}/{@entities}
default entities: {@key}={@value} separated \"_\"
";

const SOURCES: &str = "sources:\n    table[sheet=a]\n";

/// The pipeline `HEADER` and `rest` make, bound over `SOURCES`, as a
/// `.spitdag`.
fn spitdag(rest: &str) -> String {
    let pipeline = parse_pipeline(&format!("{HEADER}{rest}")).unwrap();
    let inventory = parse_source_inventory(SOURCES).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    bind_dag(&pipeline, &dag).unwrap().to_json()
}

/// Every diagnostic for the pipeline `HEADER` and `rest` make.
fn diagnostics(rest: &str) -> Vec<String> {
    rendered(&diagnose(&format!("{HEADER}{rest}"), Some(SOURCES)))
}

/// The fingerprint of the first job in `json`.
fn fingerprint(json: &str) -> String {
    let start = json.find("\"fingerprint\":\"").unwrap() + 15;
    json[start..start + 16].to_owned()
}

#[test]
fn a_job_carries_its_cores_and_memory_and_its_command_the_cores() {
    let json = spitdag(
        "operation sort(lines: Table) -> Table .tsv
command sort: sort --parallel={@cores} -o {@output} {lines}
verify sort: check_threads {@cores} {lines}
resources sort: cores=4, memory=1536M
sorted = sort(table)
operation copy(lines: Table) -> Table .tsv
command copy: cp {lines} {@output}
copied = copy(sorted)
",
    );
    assert!(
        json.contains("\"command\":[[\"sort\"],[\"--parallel=\",\"4\"],[\"-o\"],[{\"path\":\"out/sorted/sheet=a.tsv\"}],[{\"path\":\"in/a.tsv\"}]]"),
        "{json}"
    );
    assert!(
        json.contains("\"verify\":[[[\"check_threads\"],[\"4\"],[{\"path\":\"in/a.tsv\"}]]]"),
        "{json}"
    );
    assert!(
        json.contains("\"checks\":[],\"cores\":4,\"memory\":1610612736}"),
        "{json}"
    );
    // An operation that declares nothing needs one core and claims no memory.
    assert!(
        json.contains("\"checks\":[],\"cores\":1,\"memory\":null}"),
        "{json}"
    );
}

#[test]
fn resources_leave_the_fingerprint_unless_the_command_uses_them() {
    let job = |command: &str, resources: &str| {
        fingerprint(&spitdag(&format!(
            "operation sort(lines: Table) -> Table .tsv\ncommand sort: {command}\n{resources}sorted = sort(table)\n"
        )))
    };
    let plain = job("sort -o {@output} {lines}", "");
    assert_eq!(
        plain,
        job(
            "sort -o {@output} {lines}",
            "resources sort: cores=8, memory=4G\n"
        )
    );
    // `{@cores}` is 1 without a line, the same command as one that writes 1.
    let one = job("sort --parallel 1 -o {@output} {lines}", "");
    assert_eq!(
        one,
        job("sort --parallel {@cores} -o {@output} {lines}", "")
    );
    assert_ne!(
        one,
        job(
            "sort --parallel {@cores} -o {@output} {lines}",
            "resources sort: cores=2\n"
        )
    );
}

#[test]
fn the_plan_shows_what_a_job_needs_above_its_command() {
    let tree = support::Tree::new("resources", &["in/a.tsv"]);
    let pipeline = tree.write(
        "p.spit",
        &format!(
            "{HEADER}operation sort(lines: Table) -> Table .tsv\ncommand sort: sort --parallel={{@cores}} -o {{@output}} {{lines}}\nresources:\n    sort: memory=16G, cores=8\nsorted = sort(table)\noperation copy(lines: Table) -> Table .tsv\ncommand copy: cp {{lines}} {{@output}}\ncopied = copy(sorted)\n"
        ),
    );
    let inventory = tree.write("p.spitout", "root .\nsources:\n    table[sheet=a]\n");
    let output = support::spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        inventory.to_str().unwrap(),
        "--commands",
    ]);
    let printed = support::text(&output.stdout);
    assert!(output.status.success(), "{}", support::text(&output.stderr));
    assert_eq!(
        printed,
        "Job 1  sort\n  needs:  cores=8, memory=16G\n  run:    sort --parallel=8 -o out/sorted/sheet=a.tsv in/a.tsv\n\nJob 2  copy\n  run:    cp out/sorted/sheet=a.tsv out/copied/sheet=a.tsv\n"
    );
}

#[test]
fn a_malformed_line_says_what_to_write() {
    for (line, message) in [
        (
            "resources sort: cores=0",
            "line 7: `cores=0`: a job uses at least one core; leave `cores` out for one",
        ),
        (
            "resources sort: cores=two",
            "`cores=two`: give the cores a job uses as a whole number, as in `cores=8`",
        ),
        (
            "resources sort: memory=16GB",
            "`memory=16GB`: write `memory=16G`; the units are K, M, G and T, each 1024 times the one before",
        ),
        (
            "resources sort: memory=16",
            "`memory=16`: give it a unit, as in `memory=16M` or `memory=16G`",
        ),
        (
            "resources sort: threads=4",
            "`threads` is not a resource; a job declares `cores=N` and `memory=SIZE`",
        ),
        (
            "resources sort: cores=2, cores=4",
            "`resources sort` gives `cores` twice",
        ),
        (
            "resources sort: cores=2\nresources sort: memory=1G",
            "line 8: operation `sort` has a second `resources` line; give its cores and memory on one",
        ),
        (
            "resources nope: cores=2",
            "line 7: `resources` refers to unknown operation `nope`",
        ),
        (
            "resources sort",
            "expected `resources operation: cores=N, memory=SIZE`",
        ),
    ] {
        let found = diagnostics(&format!(
            "operation sort(lines: Table) -> Table .tsv\ncommand sort: sort -o {{@output}} {{lines}}\n{line}\nsorted = sort(table)\n"
        ));
        assert!(
            found.len() == 1 && found[0].contains(message),
            "{line}: {found:?}"
        );
    }
}

#[test]
fn cores_is_a_number_a_command_or_verify_takes() {
    for (lines, message) in [
        (
            "command sort: sort -o {@output.dir} {@cores.dir} {lines}",
            "`{@cores.dir}`: `{@cores}` is a number, the cores the job uses, and gives no `.dir` or `.stem`",
        ),
        (
            "command sort: sort {lines} > {@cores}",
            "`> {@cores}`: `{@cores}` is the number of cores the job uses, not a file; a command's stdout is written to one of its operation's outputs",
        ),
        (
            "command sort: sort -o {@output} {lines}\ncheck threads: test {@cores} {@path}",
            "`{@cores}`",
        ),
    ] {
        let found = diagnostics(&format!(
            "operation sort(lines: Table) -> Table .tsv\n{lines}\nsorted = sort(table)\n"
        ));
        assert!(
            !found.is_empty() && found[0].contains(message),
            "{lines}: {found:?}"
        );
    }
}

#[test]
fn an_operation_carried_out_by_steps_takes_no_resources() {
    let found = diagnostics(
        "operation sort(lines: Table) -> Table .tsv
command sort: sort -o {@output} {lines}
operation twice(lines: Table) -> (result: Table):
    once = sort(lines)
    result = sort(once)
resources twice: cores=2
sorted = twice(table)
",
    );
    assert!(
        found.len() == 1
            && found[0].contains("operation `twice` is carried out by the steps in its body, so it takes no `resources` line; give each step's operation its own"),
        "{found:?}"
    );
}

#[test]
fn an_imported_operation_brings_its_resources() {
    let tree = support::Tree::new("resources-import", &["in/a.tsv"]);
    tree.write(
        "lib.spit",
        "operation sort(lines: Table) -> Table .tsv\ncommand sort: sort --parallel={@cores} -o {@output} {lines}\nresources sort: cores=3\n",
    );
    let pipeline = |extra: &str| {
        tree.write(
            "p.spit",
            &format!("use sort from lib.spit as L\n{HEADER}{extra}sorted = L::sort(table)\n"),
        )
    };
    tree.write("p.spitout", "root .\nsources:\n    table[sheet=a]\n");
    let run = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .current_dir(tree.path())
            .output()
            .unwrap();
        (
            output.status.success(),
            support::text(&output.stdout),
            support::text(&output.stderr),
        )
    };
    pipeline("");
    let (ok, printed, errors) = run(&["dag", "p.spit", "p.spitout", "--commands"]);
    assert!(ok, "{errors}");
    assert!(
        printed.contains("  needs:  cores=3\n  run:    sort --parallel=3 "),
        "{printed}"
    );
    // A second line, in the file that imports the operation, is an error.
    pipeline("resources L::sort: cores=8\n");
    let (ok, _, errors) = run(&["check", "p.spit"]);
    assert!(!ok);
    assert!(
        errors.contains("operation `L::sort` has a second `resources` line"),
        "{errors}"
    );
}
