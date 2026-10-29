//! A `command` written as a block, its lines beneath its header: what each
//! line may use, and how a job carries the lines to a backend.

mod support;

use support::rendered;

use spit::{bind_dag, diagnose, parse_pipeline, parse_source_inventory, resolve};

const HEADER: &str = "source sam : Sam .sam [sample]
path sam: in/{sample}
default path: out/{@product}/{@entities}
default entities: {@key}={@value} separated \"_\"
";

const SOURCES: &str = "sources:\n    sam[sample=A]\n";

const SORT: &str = "operation sort(sam: Sam) -> (bam: Bam .bam, bai: Bai .bam.bai beside bam)
resources sort: cores=4
command sort:
    samtools sort --threads {@cores} -o {bam} {sam}
    # The index is written beside the BAM.
    samtools index {bam}
bam, bai = sort(sam)
";

/// The pipeline `HEADER` and `rest` make, bound over `SOURCES`, as a
/// `.spitdag`.
fn spitdag(rest: &str) -> String {
    let pipeline = parse_pipeline(&format!("{HEADER}{rest}")).unwrap();
    let inventory = parse_source_inventory(SOURCES).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    bind_dag(&pipeline, &dag).unwrap().to_json()
}

/// Every diagnostic for the pipeline `HEADER` and `rest` make, as the
/// command line prints it.
fn diagnostics(rest: &str) -> Vec<String> {
    rendered(&diagnose(&format!("{HEADER}{rest}"), Some(SOURCES)))
}

#[test]
fn a_job_runs_each_line_as_a_command_of_its_own() {
    let json = spitdag(SORT);
    assert!(
        json.contains(
            "\"commands\":[\
{\"command\":[[\"samtools\"],[\"sort\"],[\"--threads\"],[\"4\"],[\"-o\"],[{\"path\":\"out/bam/sample=A.bam\"}],[{\"path\":\"in/A.sam\"}]],\"stdin\":null,\"stdout\":null},\
{\"command\":[[\"samtools\"],[\"index\"],[{\"path\":\"out/bam/sample=A.bam\"}]],\"stdin\":null,\"stdout\":null}\
],\"verify\":[]"
        ),
        "{json}"
    );
    assert!(json.contains("\"executables\":[\"samtools\"]"), "{json}");
}

#[test]
fn a_block_of_one_line_is_the_same_job_as_the_line_written_after_the_colon() {
    let inline = spitdag(
        "operation view(sam: Sam) -> Bam .bam\ncommand view: samtools view -b {sam} > {@output}\nbam = view(sam)\n",
    );
    let block = spitdag(
        "operation view(sam: Sam) -> Bam .bam\ncommand view:\n    samtools view -b {sam} > {@output}\nbam = view(sam)\n",
    );
    assert_eq!(inline, block);
}

#[test]
fn a_line_indented_further_goes_on_with_the_line_above() {
    let json = spitdag(&SORT.replace(
        "samtools index {bam}\n",
        "samtools index\n        -@ {@cores}\n        {bam}\n",
    ));
    assert!(
        json.contains(
            "{\"command\":[[\"samtools\"],[\"index\"],[\"-@\"],[\"4\"],[{\"path\":\"out/bam/sample=A.bam\"}]],\"stdin\":null,\"stdout\":null}],"
        ),
        "{json}"
    );
}

#[test]
fn a_command_block_entry_ending_in_a_colon_holds_lines() {
    let block = SORT.replace(
        "command sort:\n    samtools sort --threads {@cores} -o {bam} {sam}\n    # The index is written beside the BAM.\n    samtools index {bam}\n",
        "command:\n    sort:\n        samtools sort --threads {@cores} -o {bam} {sam}\n        samtools index {bam}\n",
    );
    assert_eq!(spitdag(&block), spitdag(SORT));
}

#[test]
fn every_placeholder_works_on_every_line() {
    let json = spitdag(
        "operation call(sam: Sam) -> (calls: Vcf .vcf, report: Report /)
command call:
    caller --sample {sample} --threads {@cores} {sam} > {calls}
    summarise --from {sam.sample} {calls} --into {report.dir}/{report.stem}
calls, report = call(sam)
",
    );
    assert!(
        json.contains(
            "{\"command\":[[\"caller\"],[\"--sample\"],[\"A\"],[\"--threads\"],[\"1\"],[{\"path\":\"in/A.sam\"}]],\"stdin\":null,\"stdout\":\"out/calls/sample=A.vcf\"},\
{\"command\":[[\"summarise\"],[\"--from\"],[\"A\"],[{\"path\":\"out/calls/sample=A.vcf\"}],[\"--into\"],\
[{\"dir\":\"out/report\",\"of\":\"out/report/sample=A\"},\"/\",{\"stem\":\"sample=A\",\"of\":\"out/report/sample=A\"}]],\"stdin\":null,\"stdout\":null}"
        ),
        "{json}"
    );
}

#[test]
fn the_plan_shows_a_run_line_for_each_command() {
    let tree = support::Tree::new("command_sequences", &["in/A.sam"]);
    let pipeline = tree.write("p.spit", &format!("{HEADER}{SORT}"));
    let inventory = tree.write("p.spitout", &format!("root .\n{SOURCES}"));
    let output = support::spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        inventory.to_str().unwrap(),
        "--commands",
    ]);
    let printed = support::text(&output.stdout);
    assert!(output.status.success(), "{}", support::text(&output.stderr));
    assert!(
        printed.contains(
            "  needs:  cores=4\n  run:    samtools sort --threads 4 -o out/bam/sample=A.bam in/A.sam\n  run:    samtools index out/bam/sample=A.bam\n"
        ),
        "{printed}"
    );
}

#[test]
fn every_output_is_used_by_some_line() {
    let found = diagnostics(
        &SORT
            .replace("    samtools index {bam}\n", "    samtools index {sam}\n")
            .replace("-o {bam} ", ""),
    );
    assert_eq!(
        found,
        ["error: line 7: a line of the command for `sort` must use `{bam}`"]
    );
}

#[test]
fn one_line_at_most_writes_an_output_as_its_stdout() {
    let found = diagnostics(
        "operation split(sam: Sam) -> (head: Text .txt, body: Text .txt)
command split:
    samtools view -H {sam} > {head}
    samtools view {sam} > {body}
head, body = split(sam)
",
    );
    assert_eq!(
        found,
        ["error: line 8: the command for `split` writes its stdout to an output on its line 1 already; only one of a command's lines writes an output as its stdout"]
    );
}

#[test]
fn a_stdout_output_is_read_by_the_lines_after_it_only() {
    let pipeline = |lines: &str| {
        format!("operation view(sam: Sam) -> Bam .bam\ncommand view:\n{lines}bam = view(sam)\n")
    };
    assert_eq!(
        diagnostics(&pipeline(
            "    samtools view -b {sam} > {@output}\n    samtools index {@output}\n"
        )),
        Vec::<String>::new()
    );
    assert_eq!(
        diagnostics(&pipeline(
            "    samtools quickcheck {@output}\n    samtools view -b {sam} > {@output}\n"
        )),
        ["error: line 7: `{@output}`: `@output` is the stdout of line 2 of the command, written `> {@output}`, so it does not exist yet on line 1"]
    );
    assert_eq!(
        diagnostics(&pipeline("    samtools view -b {sam} {@output} > {@output}\n")),
        ["error: line 7: `{@output}`: `@output` is the command's stdout, written `> {@output}`, so it cannot be an argument too"]
    );
}

#[test]
fn an_error_in_a_line_points_at_that_line() {
    assert_eq!(
        diagnostics(&SORT.replace("samtools index {bam}", "samtools index {bam} {nope}")),
        ["error: line 10: command for `sort` uses unknown placeholder `{nope}`: `sort` has no port `nope`, and the jobs of `bam` have `sample`"]
    );
    assert_eq!(
        diagnostics(&SORT.replace("samtools index {bam}", "samtools index {bam} | cat")),
        ["error: line 10: `|` in the command for `sort` is not read as a pipe or redirection, since commands run without a shell; quote it ('|') to pass it to the program, or run a shell with the paths as its arguments, as in sh -c 'tool \"$1\" | sort > \"$2\"' sh {input} {@output}"]
    );
}

#[test]
fn each_line_is_indented_as_the_first() {
    assert_eq!(
        diagnostics(&SORT.replace("    samtools sort", "        samtools sort")),
        ["error: line 10: indent each line of the command for `sort` as its first, or further to go on with the line above"]
    );
}

#[test]
fn verify_and_check_hold_one_command_each() {
    assert_eq!(
        diagnostics(&format!("{SORT}verify sort:\n    samtools quickcheck {{sam}}\n")),
        ["error: line 12: a `verify` line holds one command, written after its `:`; give each command a `verify sort: ...` line of its own"]
    );
    assert_eq!(
        diagnostics(&format!("{SORT}check readable:\n    test -r {{@path}}\n")),
        ["error: line 12: a check runs one command, written after its `:`, as in `check readable: test -s {@path}`"]
    );
    // Several `verify` lines are run in order, before the commands.
    let json = spitdag(&format!(
        "{SORT}verify sort: samtools quickcheck {{sam}}\nverify sort: test -s {{sam}}\n"
    ));
    assert!(
        json.contains("\"verify\":[[[\"samtools\"],[\"quickcheck\"],[{\"path\":\"in/A.sam\"}]],[[\"test\"],[\"-s\"],[{\"path\":\"in/A.sam\"}]]]"),
        "{json}"
    );
}

#[test]
fn a_command_with_no_lines_is_empty() {
    assert_eq!(
        diagnostics("operation view(sam: Sam) -> Bam .bam\ncommand view:\nbam = view(sam)\n"),
        ["error: line 6: command `view`: command template must not be empty"]
    );
}
