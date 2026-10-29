//! A command's stdin and stdout, written `< {input}` and `> {output}`: what
//! they may name, and how a job carries them to a backend.

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
/// `.spitdag`, or the binding error.
fn spitdag(rest: &str) -> Result<String, String> {
    let pipeline = parse_pipeline(&format!("{HEADER}{rest}")).map_err(|e| e.to_string())?;
    let inventory = parse_source_inventory(SOURCES).unwrap();
    let dag = resolve(&pipeline, &inventory).map_err(|e| e.to_string())?;
    let bound = bind_dag(&pipeline, &dag).map_err(|e| e.to_string())?;
    Ok(bound.to_json())
}

/// Every diagnostic for the pipeline `HEADER` and `rest` make.
fn diagnostics(rest: &str) -> Vec<String> {
    rendered(&diagnose(&format!("{HEADER}{rest}"), Some(SOURCES)))
}

#[test]
fn a_job_names_its_stdin_and_stdout_apart_from_its_arguments() {
    let json = spitdag(
        "operation count(lines: Table) -> Count .txt
command count: wc -l < {lines} > {@output}
counted = count(table)
operation first(lines: Table) -> (column: Column .tsv, log: Log .txt)
command first: cut -f1 {lines} > {column} --log {log}
column, log = first(table)
",
    )
    .unwrap();
    assert!(
        json.contains(
            "\"commands\":[{\"command\":[[\"wc\"],[\"-l\"]],\"stdin\":\"in/a.tsv\",\"stdout\":\"out/counted/sheet=a.txt\"}],\"verify\":[]"
        ),
        "{json}"
    );
    assert!(
        json.contains(
            "\"command\":[[\"cut\"],[\"-f1\"],[{\"path\":\"in/a.tsv\"}],[\"--log\"],[{\"path\":\"out/log/sheet=a.txt\"}]],\
\"stdin\":null,\"stdout\":\"out/column/sheet=a.tsv\"}]"
        ),
        "{json}"
    );
    // The output is an output like any other.
    assert!(
        json.contains("\"outputs\":{\"output\":{\"product\":\"counted\""),
        "{json}"
    );
    assert!(json.contains("\"executables\":[\"cut\",\"wc\"]"), "{json}");
}

#[test]
fn the_plan_shows_a_redirected_stream_as_a_shell_writes_it() {
    let tree = support::Tree::new("redirects", &["in/a.tsv"]);
    let pipeline = tree.write(
        "p.spit",
        &format!(
            "{HEADER}operation count(lines: Table) -> Count .txt\ncommand count: wc -l < {{lines}} > {{@output}}\ncounted = count(table)\n"
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
    assert!(
        printed.contains("  run:    wc -l < in/a.tsv > out/counted/sheet=a.txt\n"),
        "{printed}"
    );
}

#[test]
fn a_redirection_names_one_port_of_the_right_kind() {
    for (command, message) in [
        (
            "tool {lines} > {@output} > {@output}",
            "the command for `count` has a second `>`; a command has one stdout",
        ),
        (
            "tool < {lines} < {lines} > {@output}",
            "the command for `count` has a second `<`; a command has one stdin",
        ),
        (
            "tool {lines} >",
            "`>` in the command for `count` needs the output its stdout goes to after it, as in `> {@output}`",
        ),
        (
            "tool {lines} > out.txt",
            "`> out.txt` in the command for `count`: a command's stdout is connected to one of its operation's outputs",
        ),
        (
            "tool {lines} > x{@output}",
            "`> x{@output}` in the command for `count`",
        ),
        (
            "tool {lines} >{@output}",
            "`>{@output}` in the command for `count` is not read as a redirection, since commands run without a shell",
        ),
        (
            "tool {lines} >> {@output}",
            "`>>` in the command for `count` is not read as a redirection",
        ),
        (
            "tool {lines} 2> {@output}",
            "`2>` in the command for `count` is not read as a redirection",
        ),
        (
            "> {@output}",
            "command template names no program to run, only where its stdin or stdout goes",
        ),
        (
            "tool > {lines} {@output}",
            "`> {lines}`: `lines` is an input of `count`, and a command's stdout is written to one of its outputs",
        ),
        (
            "tool < {@output}",
            "`< {@output}`: `@output` is an output of `count`, and a command's stdin reads one of its inputs",
        ),
        (
            "tool {lines} > {x}",
            "command for `count` uses unknown placeholder `{x}`",
        ),
        (
            "tool {lines} > {@output.dir}",
            "`> {@output.dir}`: a command's stdin and stdout are files, so write `> {@output}`",
        ),
        (
            "tool {lines} {@output} > {@output}",
            "`{@output}`: `@output` is the command's stdout, written `> {@output}`, so it cannot be an argument too",
        ),
        (
            "tool {lines} --name {@output.stem} > {@output}",
            "`{@output.stem}`: `@output` is the command's stdout",
        ),
    ] {
        let found = diagnostics(&format!(
            "operation count(lines: Table) -> Count .txt\ncommand count: {command}\ncounted = count(table)\n"
        ));
        assert!(
            found.len() == 1 && found[0].contains(message),
            "{command}: {found:?}"
        );
    }
}

#[test]
fn stdin_is_one_file_and_stdout_is_not_a_folder() {
    let many = diagnostics(
        "operation join(sheets: many Table) -> Table .tsv\ncommand join: cat < {sheets} > {@output}\npath joined: out/joined.tsv\njoined = join(table @vary(sheet))\n",
    );
    assert!(
        many.len() == 1
            && many[0].contains(
                "`< {sheets}`: `sheets` is a many input, and a command's stdin reads one file"
            ),
        "{many:?}"
    );
    let folder = diagnostics(
        "operation unpack(archive: Table) -> Files /\ncommand unpack: tar -x < {archive} > {@output}\nfiles = unpack(table)\n",
    );
    assert!(
        folder.len() == 1
            && folder[0].contains(
                "`> {@output}`: `@output` is a folder, and a command's stdout is one file"
            ),
        "{folder:?}"
    );
    // Whether a source is a folder is known from its artifacts' product.
    let pipeline = parse_pipeline(&format!(
        "{HEADER}source dicom : Dicom / [sheet]\npath dicom: dicom/{{sheet}}\noperation count(series: Dicom) -> Count .txt\ncommand count: wc -c < {{series}} > {{@output}}\ncounted = count(dicom)\n"
    ))
    .unwrap();
    let inventory = parse_source_inventory("sources:\n    dicom[sheet=a]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let error = bind_dag(&pipeline, &dag).unwrap_err().to_string();
    assert_eq!(
        error,
        "`< {series}` in the command for `count` reads `dicom[sheet=a]` on its stdin, but it is a folder, and stdin reads one file"
    );
}

#[test]
fn only_an_operations_command_redirects() {
    let verify = diagnostics(
        "operation count(lines: Table) -> Count .txt\ncommand count: wc -l < {lines} > {@output}\nverify count: test < {lines}\ncounted = count(table)\n",
    );
    assert!(
        verify.len() == 1
            && verify[0].contains("`<` in the verify command for `count` is not read as a redirection: only an operation's `command` connects its stdin to an input"),
        "{verify:?}"
    );
    let check = diagnostics("check big: test -s {@path} > {@path}\n");
    assert!(
        check.len() == 1
            && check[0].contains("`>` in check `big` is not read as a redirection: only an operation's `command` connects its stdout to an output"),
        "{check:?}"
    );
    // Quoted, each is an argument.
    let quoted = diagnostics(
        "operation count(lines: Table) -> Count .txt\ncommand count: tool '<' {lines} \">\" {@output}\ncounted = count(table)\n",
    );
    assert!(quoted.is_empty(), "{quoted:?}");
}

#[test]
fn a_redirected_output_changes_the_jobs_fingerprint() {
    let fingerprint = |command: &str| {
        let json = spitdag(&format!(
            "operation count(lines: Table) -> Count .txt\ncommand count: {command}\ncounted = count(table)\n"
        ))
        .unwrap();
        let start = json.find("\"fingerprint\":\"").unwrap() + 15;
        json[start..start + 16].to_owned()
    };
    let argument = fingerprint("tool {lines} {@output}");
    let stdout = fingerprint("tool {lines} > {@output}");
    let stdin = fingerprint("tool < {lines} > {@output}");
    assert_ne!(argument, stdout);
    assert_ne!(stdout, stdin);
}
