//! A command's `{dimension}` placeholders: each job's value filled in as
//! text, and the errors for a dimension its jobs have no one value of.

mod support;

use support::rendered;

use spit::{bind_dag, diagnose, parse_pipeline, parse_source_inventory, resolve};

const HEADER: &str = "source cell : Cell .tsv [sheet, row]
path cell: in/{sheet}/{row}
default path: out/{@product}/{@entities}
default entities: {@key}={@value} separated \"_\"
";

const SOURCES: &str = "sources:
    cell[sheet=a,row=1]
    cell[sheet=a,row=2]
    cell[sheet=b,row=1]
";

/// The pipeline `HEADER` and `rest` make, bound over `SOURCES`, as a
/// `.spitdag`.
fn spitdag(rest: &str) -> String {
    let pipeline = parse_pipeline(&format!("{HEADER}{rest}")).unwrap();
    let inventory = parse_source_inventory(SOURCES).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    bind_dag(&pipeline, &dag).unwrap().to_json()
}

/// Every diagnostic for the pipeline `HEADER` and `rest` make, each with
/// the text it points at.
fn diagnostics(rest: &str) -> Vec<String> {
    let text = format!("{HEADER}{rest}");
    let found = diagnose(&text, Some(SOURCES));
    rendered(&found)
        .into_iter()
        .zip(&found)
        .map(|(line, diagnostic)| {
            let at = diagnostic
                .line
                .and_then(|number| text.lines().nth(number - 1))
                .zip(diagnostic.columns.clone())
                .map_or("", |(content, columns)| &content[columns]);
            format!("{line} (at `{at}`)")
        })
        .collect()
}

/// Each job's fingerprint in `json`, in order.
fn fingerprints(json: &str) -> Vec<&str> {
    json.match_indices("\"fingerprint\":\"")
        .map(|(at, key)| &json[at + key.len()..at + key.len() + 16])
        .collect()
}

#[test]
fn a_command_and_its_verify_are_given_the_jobs_values_as_text() {
    let json = spitdag(
        "operation label(cell: Cell) -> Label .txt
command label: tag --name \"sheet {sheet}\" --row={row} {cell} {@output}
verify label: check {sheet} {cell}
labelled = label(cell)
",
    );
    // The value is text within its argument, which it never splits.
    assert!(
        json.contains("\"command\":[[\"tag\"],[\"--name\"],[\"sheet \",\"a\"],[\"--row=\",\"2\"],[{\"path\":\"in/a/2.tsv\"}]"),
        "{json}"
    );
    assert!(
        json.contains("\"verify\":[[[\"check\"],[\"b\"],[{\"path\":\"in/b/1.tsv\"}]]]"),
        "{json}"
    );
}

#[test]
fn a_value_joins_the_fingerprint_as_the_same_text_written_out_would() {
    let first = |command: &str| {
        let json = spitdag(&format!(
            "operation label(cell: Cell) -> Label .txt\ncommand label: {command}\nlabelled = label(cell)\n"
        ));
        let jobs = fingerprints(&json);
        // Jobs that differ only in their values differ in fingerprint.
        assert_eq!(jobs.len(), 3);
        jobs[0].to_owned()
    };
    let filled = first("tag {sheet} {row} {cell} {@output}");
    assert_eq!(filled, first("tag a 1 {cell} {@output}"));
    assert_ne!(filled, first("tag {cell} {@output}"));
}

#[test]
fn an_aggregate_gives_the_dimensions_it_keeps() {
    let json = spitdag(
        "operation table(cells: many Cell) -> Table .csv
command table: table --sheet {sheet} {cells} -o {@output}
tabled = table(cell @vary(row))
",
    );
    assert!(
        json.contains("\"command\":[[\"table\"],[\"--sheet\"],[\"b\"],[{\"path\":\"in/b/1.tsv\"}]"),
        "{json}"
    );
}

#[test]
fn a_dimension_the_jobs_lack_is_an_error_at_the_placeholder() {
    let lacks = "operation table(cells: many Cell) -> Table .csv
command table: table --sheet {sheet} {cells} -o {@output}
path tabled: out/tabled.csv
tabled = table(cell @vary(sheet, row))
";
    assert_eq!(
        diagnostics(lacks),
        ["error: line 6: command for `table` uses `{sheet}`, but `cells` collects `cell` across `sheet` with `@vary(sheet, row)`, so each job has many values of it; the jobs of `tabled` have no dimensions (at `{sheet}`)"],
    );
    let pinned = "source lookup : Lookup [version]
operation join(cell: Cell, lookup: Lookup) -> Cell
command join: join --version {version} {cell} {lookup} {@output}
joined = join(cell, lookup @where(version=2))
";
    assert_eq!(
        diagnostics(pinned),
        ["error: line 7: command for `join` uses dimension `{version}`, which its jobs lack: the jobs of `joined` have `sheet`, `row` (at `{version}`)"],
    );
    let typo = "operation label(cell: Cell) -> Label .txt
command label: tag {shet} {cell} {@output}
labelled = label(cell)
";
    assert_eq!(
        diagnostics(typo),
        ["error: line 6: command for `label` uses unknown placeholder `{shet}`: `label` has no port `shet`, and the jobs of `labelled` have `sheet`, `row` (at `{shet}`)"],
    );
}

#[test]
fn every_step_calling_the_operation_must_have_the_dimension() {
    let text = "operation label(cell: Cell) -> Label .txt
command label: tag {row} {cell} {@output}
labelled = label(cell)
operation squash(cells: many Cell) -> Cell .tsv
command squash: cat {cells} > {@output}
squashed = squash(cell @vary(row))
relabelled = label(squashed)
";
    assert_eq!(
        diagnostics(text),
        ["error: line 6: command for `label` uses dimension `{row}`, which its jobs lack: the jobs of `relabelled` have `sheet` (at `{row}`)"],
    );
}

#[test]
fn an_operation_no_step_calls_is_checked_when_one_does() {
    let uncalled = "operation label(cell: Cell) -> Label .txt
command label: tag {subject} {cell} {@output}
";
    assert!(
        diagnostics(uncalled)
            .iter()
            .all(|line| !line.starts_with("error")),
        "{:?}",
        diagnostics(uncalled)
    );
    assert_eq!(
        diagnostics(&format!("{uncalled}labelled = label(cell)\n")),
        ["error: line 6: command for `label` uses unknown placeholder `{subject}`: `label` has no port `subject`, and the jobs of `labelled` have `sheet`, `row` (at `{subject}`)"],
    );
}

#[test]
fn a_port_comes_before_a_dimension_of_the_same_name() {
    let json = spitdag(
        "operation label(sheet: Cell) -> Label .txt
command label: tag {sheet} {@output}
labelled = label(cell)
",
    );
    assert!(
        json.contains("\"command\":[[\"tag\"],[{\"path\":\"in/a/1.tsv\"}]"),
        "{json}"
    );
}

#[test]
fn a_dimension_is_text_not_a_file() {
    for (command, error) in [
        (
            "tag {sheet.dir} {cell} {@output}",
            "command for `label` uses unknown placeholder `{sheet.dir}`; only an operation's outputs give `.dir` and `.stem`",
        ),
        (
            "tag {cell} > {sheet}",
            "command for `label` uses unknown placeholder `{sheet}`",
        ),
        (
            "tag < {row} {@output}",
            "command for `label` uses unknown placeholder `{row}`",
        ),
    ] {
        let found = diagnostics(&format!(
            "operation label(cell: Cell) -> Label .txt\ncommand label: {command}\nlabelled = label(cell)\n"
        ));
        assert!(
            found.iter().any(|line| line.contains(error)),
            "{command}: {found:?}"
        );
    }
}

#[test]
fn a_call_gives_the_steps_it_makes_the_callers_dimensions() {
    let text = "operation label(cell: Cell) -> Label .txt
command label: tag {sheet} {cell} {@output}
operation tidy(x: Cell) -> (y: Label):
    y = label(x)
first = tidy(cell)
";
    let json = spitdag(text);
    assert!(
        json.contains("\"command\":[[\"tag\"],[\"a\"],[{\"path\":\"in/a/1.tsv\"}]"),
        "{json}"
    );
}
