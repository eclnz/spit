//! Keyword blocks, such as `source:`, whose lines are statements of that
//! keyword written without it, and statements that go on over several
//! lines: a command over the lines indented beneath it, and any statement
//! while its brackets are open.

mod support;

use spit::{diagnose, parse_pipeline, parse_source_inventory, resolve, View};

/// The jobs `text` plans over one run of `bold` and one `t1w`, with their
/// paths and commands.
fn plan(text: &str) -> String {
    let pipeline = parse_pipeline(text).unwrap();
    let inventory =
        parse_source_inventory("sources:\n    bold[sub=1,run=1]\n    t1w[sub=1]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let bound = spit::bind_dag(&pipeline, &dag).unwrap();
    spit::render_bound_dag(
        &bound,
        View {
            paths: true,
            commands: true,
        },
    )
}

/// The errors in `text`, each with its line.
fn errors(text: &str) -> Vec<(usize, String)> {
    support::errors(diagnose(text, None))
        .into_iter()
        .map(|error| (error.line.unwrap_or(0), error.message))
        .collect()
}

const ONE_PER_LINE: &str = "\
source bold : Bold [sub, run]
source t1w : T1w [sub]
path bold: in/{sub}/{run}.bold
path t1w: in/{sub}/t1w.img
check nonempty: test -s {@path}
operation denoise(bold: Bold) -> Bold .img
operation register(bold: Bold, anatomy: T1w) -> Bold .img
command denoise: dn {bold} {@output}
command register: flirt -in {bold} -ref {anatomy} -out {@output}
verify register: same_grid {bold} {anatomy}
default check: nonempty
denoised = denoise(bold)
registered = register(denoised, t1w)
default entities: {@key}={@value} separated \"__\"\npath denoised: out/{@product}/{@entities}\npath registered: out/{@product}/{@entities}\n";

const BLOCKS: &str = "\
source:
    bold : Bold [sub, run]
    t1w : T1w [sub]

path:
    bold: in/{sub}/{run}.bold
    t1w: in/{sub}/t1w.img

check:
    nonempty: test -s {@path}

operation:
    denoise(bold: Bold) -> Bold .img
    register(
        bold: Bold,
        anatomy: T1w
    ) -> Bold .img

command:
    denoise: dn {bold} {@output}
    register: flirt -in {bold}
        -ref {anatomy}

        -out {@output}

verify:
    register: same_grid {bold} {anatomy}

default check: nonempty
denoised = denoise(bold)
registered = register(
    denoised,
    t1w
)
default entities: {@key}={@value} separated \"__\"\npath denoised: out/{@product}/{@entities}\npath registered: out/{@product}/{@entities}\n";

#[test]
fn a_keyword_block_reads_as_its_statements_written_one_per_line() {
    assert_eq!(errors(BLOCKS), []);
    assert_eq!(plan(BLOCKS), plan(ONE_PER_LINE));
}

#[test]
fn an_operation_block_may_hold_an_operation_carried_out_by_steps() {
    let text = "source:\n    raw : T [id]\npath raw: in/{id}.txt\noperation:\n    step(input: T) -> T .txt\n    twice(input: T) -> (out: T):\n        once = step(input)\n        out = step(once)\ncommand step: cp {input} {@output}\nresult = twice(raw)\ndefault entities: {@key}={@value} separated \"__\"\npath result: out/{@product}/{@entities}\npath result::once: out/{@product}/{@entities}\n";
    assert_eq!(errors(text), []);
    let pipeline = parse_pipeline(text).unwrap();
    let twice = pipeline
        .operations
        .iter()
        .find(|operation| operation.name == "twice")
        .unwrap();
    assert_eq!(twice.steps.len(), 2);
}

/// Each entry is read as its keyword's statement: an error in one is on its
/// own line, and a statement that belongs at the top level does there too.
#[test]
fn an_entry_fails_as_its_keywords_statement_would() {
    let text = "source:\n    good : T [id]\n    bad : T [id\n    also_good : T [id]\nstage s:\n    source:\n        inner : T [id]\n";
    let found = errors(text);
    let lines: Vec<_> = found.iter().map(|(line, _)| *line).collect();
    assert_eq!(lines, [3, 7], "{found:?}");
    assert!(
        found[1]
            .1
            .contains("belongs at the top level, outside stage `s`"),
        "{found:?}"
    );
}

#[test]
fn the_entries_of_a_block_share_one_indentation() {
    let text = "source:\n    a : T [id]\n      b : T [id]\n";
    assert_eq!(
        errors(text),
        [(
            3,
            "this line is indented differently from the other lines of `source:`".to_owned()
        )]
    );
}

/// Only a command goes on over the lines indented beneath it. Beneath any
/// other statement, an indented line is read on its own, as before.
#[test]
fn only_a_command_goes_on_over_indented_lines() {
    let text = "source raw : T [id]\npath raw: in/{id}.txt\noperation copy(input: T) -> T .txt\ncommand copy: cp\n    {input}\n    {@output}\ndefault path: out/{@product}/{@entities}\n    copied = copy(raw)\ndefault entities: {@key}={@value} separated \"__\"\n";
    assert_eq!(errors(text), []);
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(
        pipeline.commands[0].lines[0].as_str(),
        "cp\n    {input}\n    {@output}"
    );
    assert_eq!(pipeline.invocations.len(), 1);
}

/// A step goes on while its brackets are open, and a step after one that
/// leaves them open is still a step of its own.
#[test]
fn a_step_goes_on_while_its_brackets_are_open() {
    let text = "source raw : T [id]\noperation pair(a: T, b: T) -> T\nboth = pair(\n    raw,\n    raw @where(id=1)\n)\nbroken = pair(raw,\nnext = pair(raw, raw)\n";
    let found = errors(text);
    let lines: Vec<_> = found.iter().map(|(line, _)| *line).collect();
    assert_eq!(lines, [7], "{found:?}");
}

/// An empty block opens nothing to read, and a bare keyword that opens no
/// block is no statement.
#[test]
fn a_block_with_no_entries_is_no_error() {
    assert_eq!(errors("source:\nsource raw : T [id]\n"), []);
}

#[test]
fn a_recipe_writes_its_rules_and_source_paths_in_blocks() {
    let recipe = "pipeline p.spit\nroot data\ninputs:\n    raw: in/{id}.txt\nrequire:\n    [id] where raw count=1\nexclude:\n    [id=2]\n";
    let rules = spit::parse_input_spec(recipe).unwrap().rules;
    assert_eq!(rules.source_paths["raw"].as_str(), "in/{id}.txt");
    assert_eq!(rules.constraints.len(), 1);
    assert_eq!(rules.constraints[0].product, "raw");
    assert_eq!(rules.constraints[0].group_by, ["id"]);
    assert_eq!(rules.exclusions.len(), 1);
    assert_eq!(
        rules.exclusions[0].values,
        [("id".to_owned(), "2".to_owned())]
    );
}

#[test]
fn the_editor_knows_a_block_entry_and_a_continued_line_by_their_keyword() {
    let words = spit::builtin_words(BLOCKS);
    let output: Vec<_> = words
        .iter()
        .filter(|word| word.word == spit::Word::Output)
        .map(|word| word.line)
        .collect();
    // `{@output}` in `denoise`'s entry, and on `register`'s last line.
    assert_eq!(output, [20, 24]);
}
