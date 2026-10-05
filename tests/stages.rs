mod support;

use support::bound;

use std::fs;
use std::process::Command;

use spit::{diagnose, inspect_paths, parse_pipeline, render_dag, resolve, Diagnostic, PathRule};

const PIPELINE: &str = "examples/stages/stages.spit";
const SOURCES: &str = "examples/stages/stages.spitout";

fn staged() -> String {
    fs::read_to_string(PIPELINE).unwrap() + &records(SOURCES)
}

/// A `.spitout`'s records, without the `root` line that comes before every
/// section, so they can follow a pipeline in one text.
fn records(file: &str) -> String {
    fs::read_to_string(file)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("root "))
        .map(|line| format!("{line}\n"))
        .collect()
}

fn messages(diagnostics: &[Diagnostic]) -> Vec<(Option<usize>, &str)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.line, diagnostic.message.as_str()))
        .collect()
}

fn spit(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_spit"))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn steps_belong_to_the_stage_whose_block_holds_them() {
    let (pipeline, _) = support::parse_fixture(&staged()).unwrap();
    let names: Vec<_> = pipeline.stages.iter().map(|stage| &stage.name).collect();
    assert_eq!(names, ["preprocess", "analysis"]);
    assert_eq!(pipeline.stage_of("sorted"), Some("preprocess"));
    assert_eq!(pipeline.stage_of("merged"), Some("preprocess"));
    assert_eq!(pipeline.stage_of("tally"), Some("analysis"));
    assert_eq!(pipeline.stage_of("shard"), None);
    // Operations stay global.
    assert_eq!(pipeline.operations.len(), 3);
}

#[test]
fn an_unindented_line_ends_a_stage() {
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage first:\n    a = copy(raw)\n\n    # a comment keeps the stage open\n    b = copy(a)\nc = copy(b)\n";
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(pipeline.stage_of("a"), Some("first"));
    assert_eq!(pipeline.stage_of("b"), Some("first"));
    assert_eq!(pipeline.stage_of("c"), None);
}

#[test]
fn a_product_named_stage_is_still_a_step() {
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage = copy(raw)\n";
    let pipeline = parse_pipeline(text).unwrap();
    assert!(pipeline.stages.is_empty());
    assert_eq!(pipeline.invocations[0].outputs, ["stage"]);
}

#[test]
fn jobs_carry_their_stage() {
    let (pipeline, inventory) = support::parse_fixture(&staged()).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    let stages: Vec<_> = dag
        .jobs
        .iter()
        .map(|job| dag.step(job).stage.as_deref())
        .collect();
    assert_eq!(
        stages,
        [
            Some("preprocess"),
            Some("preprocess"),
            Some("preprocess"),
            Some("preprocess"),
            Some("preprocess"),
            Some("analysis"),
            Some("analysis"),
        ]
    );
    assert!(render_dag(&dag).contains("Job 6\n  stage: analysis\n  operation: tally_lines"));
}

#[test]
fn a_stage_path_rule_covers_only_that_stage() {
    let (pipeline, inventory) = support::parse_fixture(&staged()).unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    let rule = |product: &str| {
        coverage
            .entries
            .iter()
            .find(|entry| entry.product == product)
            .map(|entry| entry.rule.clone())
            .unwrap()
    };
    assert_eq!(
        rule("merged"),
        PathRule::Default("{@stage}/{@product}/{@entities}.txt".to_owned())
    );
    assert_eq!(
        rule("tally"),
        PathRule::Stage {
            stage: "analysis".to_owned(),
            template: "results/{@product}/{@entities}.txt".to_owned(),
        }
    );
    coverage.validate(["merged", "tally"]).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    let bound = bound(&pipeline, &dag).unwrap();
    assert!(bound.contains("path: preprocess/merged/group=alpha.txt"));
    assert!(bound.contains("path: results/tally/group=alpha.txt"));
}

#[test]
fn stage_placeholder_needs_a_stage() {
    let text = "path: {@stage}/{@product}/{@entities}\nsource raw [id]\npath raw: in/{id}\noperation copy(a: A) -> A\nloose = copy(raw)\n";
    let diagnostics = diagnose(text, None);
    assert_eq!(
        messages(&diagnostics),
        [(
            Some(1),
            "path template for `loose` uses `{@stage}`, but `loose` is not made in a stage; write `[{@stage}/]` to leave the stage's directory out for products outside every stage"
        )]
    );
    // The fix the message gives works.
    let fixed = text.replace("path: {@stage}/", "path: [{@stage}/]");
    assert_eq!(messages(&diagnose(&fixed, None)), []);
}

#[test]
fn stages_may_read_from_each_other_in_a_cycle() {
    // A stage groups steps and scopes their paths; jobs are ordered by what
    // they read, so `first` and `third` may each read from the other.
    let text = "source raw [id]\noperation copy(a: A) -> A\noperation pair(a: A, a2: A) -> A\nstage first:\n    a = copy(raw)\n    d = pair(a, late)\nstage second:\n    b = copy(a)\nglue = copy(b)\nstage third:\n    late = copy(glue)\n";
    assert_eq!(messages(&diagnose(text, None)), []);
    let (pipeline, _) = support::parse_fixture(text).unwrap();
    let inventory = spit::parse_source_inventory("sources:\n    raw[id=1]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let order: Vec<_> = dag
        .jobs
        .iter()
        .map(|job| dag.steps[job.step.index()].stage.as_deref())
        .collect();
    assert_eq!(
        order,
        [
            Some("first"),
            Some("second"),
            None,
            Some("third"),
            Some("first")
        ]
    );
}

#[test]
fn stages_that_only_read_forward_are_accepted() {
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage first:\n    a = copy(raw)\nstage second:\n    b = copy(a)\nstage third:\n    c = copy(a)\n    d = copy(b)\n";
    assert!(diagnose(text, None).is_empty());
}

#[test]
fn stage_syntax_errors() {
    let cases = [
        (
            "stage prep:\n    source raw [id]\n",
            (
                Some(2),
                "`source`, which declares an input, belongs at the top level, outside stage `prep`",
            ),
        ),
        (
            "source raw [id]\nstage prep:\n    require [id] where raw count>=1\n",
            (
                Some(3),
                "`require`, which checks sources, belongs at the top level, outside stage `prep`",
            ),
        ),
        (
            "  stage prep:\n",
            (
                Some(1),
                "a stage header outside every stage starts at the beginning of its line",
            ),
        ),
        (
            "source raw [id]\noperation copy(a: A) -> A\nstage prep:\n    a = copy(raw)\n      b = copy(a)\n",
            (
                Some(5),
                "this line is indented differently from the other lines of stage `prep`",
            ),
        ),
        (
            "source raw [id]\noperation copy(a: A) -> A\nstage prep:\n    stage inner:\n        a = copy(raw)\n  b = copy(a)\n",
            (
                Some(6),
                "this line is indented differently from the other lines of stage `prep`",
            ),
        ),
        (
            "stage a:\n    operation copy(x: Text) -> Text\nstage b:\n    operation copy(x: Text) -> Text\n",
            (Some(4), "duplicate operation `copy`: it is already declared on line 2; operations are global even when declared in a stage, so give this one another name"),
        ),
        (
            "operation copy(x: Text) -> Text\noperation copy(x: Text) -> Text\n",
            (Some(2), "duplicate operation `copy`: it is already declared on line 1"),
        ),
        (
            "stage prep\n",
            (
                Some(1),
                "expected `stage name:`, with the stage's lines indented beneath it",
            ),
        ),
        (
            "stage prep:\n    path: a/{@product}/{@entities}\n    path: b/{@product}/{@entities}\n",
            (Some(3), "duplicate default path template for stage `prep`"),
        ),
        (
            "stage prep:\n    path: a/{@product}/{@entities}\nstage prep:\n    path: b/{@product}/{@entities}\n",
            (Some(4), "duplicate default path template for stage `prep`"),
        ),
        (
            "stage prep:\n    ext: .txt\nstage prep:\n    ext: .csv\n",
            (Some(4), "duplicate `ext:` for stage `prep`"),
        ),
    ];
    for (text, expected) in cases {
        let diagnostics = diagnose(text, None);
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .cloned()
            .collect();
        assert_eq!(messages(&errors), [expected], "{text}");
    }
}

#[test]
fn a_stage_opened_again_continues_its_first_block() {
    // As in the usability study: `func` split into two blocks around a step
    // of another stage. The second block's steps join `func`, and take its
    // default path, wherever in the two blocks that is written.
    let text = "source raw [id]\npath raw: in/{@entities}\noperation copy(a: A) -> A\nstage func:\n    a = copy(raw)\nstage anat:\n    b = copy(a)\nstage func:\n    path: f/{@product}/{@entities}\n    c = copy(b)\n    stage inner:\n        d = copy(c)\nstage func:\n    stage inner:\n        e = copy(d)\n";
    assert_eq!(messages(&diagnose(text, None)), []);
    let (pipeline, _) = support::parse_fixture(text).unwrap();
    let names: Vec<_> = pipeline.stages.iter().map(|stage| &stage.name).collect();
    assert_eq!(names, ["func", "anat", "func/inner"]);
    assert_eq!(pipeline.stage_of("a"), Some("func"));
    assert_eq!(pipeline.stage_of("c"), Some("func"));
    assert_eq!(pipeline.stage_of("e"), Some("func/inner"));
    let inventory = spit::parse_source_inventory("sources:\n    raw[id=1]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let bound = bound(&pipeline, &dag).unwrap();
    for product in ["a", "c", "d", "e"] {
        assert!(
            bound.contains(&format!("path: f/{product}/id=1")),
            "{bound}"
        );
    }
    assert!(bound.contains("path: out/b/id=1"), "{bound}");
    // A header that opens a stage again is no place to report it from.
    let empty = "source raw [id]\noperation copy(a: A) -> A\nstage prep:\nstage prep:\n    a = copy(raw)\nstage idle:\nstage idle:\n";
    assert_eq!(
        messages(&diagnose(empty, None)),
        [(Some(6), "stage `idle` has no steps")]
    );
}

#[test]
fn an_empty_stage_is_reported() {
    let text =
        "source raw [id]\noperation copy(a: A) -> A\nstage prep:\n    a = copy(raw)\nstage later:\n";
    let diagnostics = diagnose(text, None);
    assert_eq!(
        messages(&diagnostics),
        [(Some(5), "stage `later` has no steps")]
    );
}

#[test]
fn check_counts_jobs_per_stage() {
    let (ok, _, stderr) = spit(&["dag", PIPELINE, SOURCES]);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("7 jobs resolved: 5 in preprocess, 2 in analysis."));
}

const NESTED: &str = "examples/stages/nested.spit";
const NESTED_SOURCES: &str = "examples/stages/nested.spitout";

fn nested() -> String {
    fs::read_to_string(NESTED).unwrap() + &records(NESTED_SOURCES)
}

#[test]
fn nested_stages_are_named_by_their_path() {
    let (pipeline, _) = support::parse_fixture(&nested()).unwrap();
    let names: Vec<_> = pipeline.stages.iter().map(|stage| &stage.name).collect();
    assert_eq!(
        names,
        [
            "preprocess",
            "preprocess/clean",
            "preprocess/combine",
            "analysis"
        ]
    );
    assert_eq!(pipeline.stage_of("sorted"), Some("preprocess/clean"));
    assert_eq!(pipeline.stage_of("merged"), Some("preprocess/combine"));
    // A line back at the outer stage's indentation closes the nested one.
    assert_eq!(pipeline.stage_of("resorted"), Some("preprocess"));
    assert_eq!(pipeline.stage_of("tally"), Some("analysis"));
}

#[test]
fn the_same_name_can_be_nested_in_different_stages() {
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage one:\n    stage part:\n        a = copy(raw)\nstage two:\n    stage part:\n        b = copy(a)\n";
    assert!(diagnose(text, None).is_empty());
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(pipeline.stage_of("b"), Some("two/part"));
}

#[test]
fn nested_stages_nest_their_paths_and_inherit_defaults() {
    let text = "path: {@stage}/{@product}/{@entities}\nsource raw [id]\npath raw: in/{id}\noperation copy(a: A) -> A\nstage outer:\n    path: out/{@stage}/{@product}/{@entities}\n    stage inner:\n        a = copy(raw)\n    stage own:\n        path: own/{@product}/{@entities}\n        b = copy(a)\n";
    let (pipeline, _) = support::parse_fixture(text).unwrap();
    let coverage = inspect_paths(&pipeline).unwrap();
    let rule = |product: &str| {
        coverage
            .entries
            .iter()
            .find(|entry| entry.product == product)
            .map(|entry| entry.rule.clone())
            .unwrap()
    };
    assert_eq!(
        rule("a"),
        PathRule::Stage {
            stage: "outer".to_owned(),
            template: "out/{@stage}/{@product}/{@entities}".to_owned(),
        }
    );
    assert_eq!(
        rule("b"),
        PathRule::Stage {
            stage: "outer/own".to_owned(),
            template: "own/{@product}/{@entities}".to_owned(),
        }
    );
    let inventory = spit::parse_source_inventory("sources:\n    raw[id=1]\n").unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    let bound = bound(&pipeline, &dag).unwrap();
    assert!(bound.contains("path: out/outer/inner/a/id=1"), "{bound}");
    assert!(bound.contains("path: own/b/id=1"), "{bound}");
}

#[test]
fn an_outer_stage_counts_the_jobs_of_the_stages_nested_in_it() {
    let (ok, _, stderr) = spit(&["dag", NESTED, NESTED_SOURCES]);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("9 jobs resolved: 7 in preprocess, 2 in analysis."));
}

#[test]
fn nested_stages_may_read_from_each_other_in_a_cycle() {
    // Siblings within one stage, through a step in the outer stage itself.
    let siblings = "source raw [id]\noperation copy(a: A) -> A\nstage outer:\n    glue : Item [id] = copy(a)\n    stage first:\n        a = copy(raw)\n        c : Item [id] = copy(b)\n    stage second:\n        b = copy(glue)\n";
    // Stages nested in two outer stages that read from each other.
    let across = "source raw [id]\noperation copy(a: A) -> A\nstage prep:\n    stage deep:\n        a : Item [id] = copy(late)\n        c = copy(raw)\nstage later:\n    stage deep:\n        late = copy(c)\n";
    for text in [siblings, across] {
        assert_eq!(messages(&diagnose(text, None)), [], "{text}");
    }
}

#[test]
fn a_nested_stage_may_read_from_its_outer_stage_and_siblings() {
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage outer:\n    base = copy(raw)\n    stage first:\n        a = copy(base)\n    stage second:\n        b = copy(a)\n    top = copy(b)\n";
    assert!(diagnose(text, None).is_empty());
}

#[test]
fn a_stage_whose_steps_are_all_nested_is_not_empty() {
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage outer:\n    stage inner:\n        a = copy(raw)\n    stage idle:\n";
    assert_eq!(
        messages(&diagnose(text, None)),
        [(Some(6), "stage `outer/idle` has no steps")]
    );
}

#[test]
fn an_operation_called_outside_its_stage_says_where_to_declare_it() {
    let declared = "source raw [id]\nstage prep:\n    stage clean:\n        operation copy(a: A) -> A\n        a = copy(raw)\n";
    let warning = |called: &str, line: usize, target: &str| {
        format!("operation `copy` is declared in stage `prep/clean` but called {called} on line {line}; a stage does not limit where its operations are used, so declare it in {target}")
    };
    let cases = [
        (
            "    stage merge:\n        b = copy(a)\n",
            warning(
                "in stage `prep/merge`",
                7,
                "stage `prep`, which holds every call",
            ),
        ),
        (
            "stage report:\n    b = copy(a)\n",
            warning("in stage `report`", 7, "the top level, outside every stage"),
        ),
        (
            "b = copy(a)\n",
            warning(
                "outside every stage",
                6,
                "the top level, outside every stage",
            ),
        ),
    ];
    for (calls, expected) in cases {
        let text = format!("{declared}{calls}");
        let diagnostics = diagnose(&text, None);
        assert_eq!(
            messages(&diagnostics),
            [(Some(4), expected.as_str())],
            "{text}"
        );
    }
}

#[test]
fn the_stage_to_declare_in_holds_every_outside_call() {
    let text = "source raw [id]\nstage a:\n    stage b:\n        operation copy(x: Text) -> Text\n        one = copy(raw)\n    stage c:\n        two = copy(one)\n    stage cd:\n        stage e:\n            three = copy(two)\n";
    assert_eq!(
        messages(&diagnose(text, None)),
        [(Some(4), "operation `copy` is declared in stage `a/b` but called in stage `a/c` on line 7; a stage does not limit where its operations are used, so declare it in stage `a`, which holds every call")]
    );
}

#[test]
fn an_operation_called_within_its_stage_is_in_place() {
    let text = "source raw [id]\nstage prep:\n    operation copy(a: A) -> A\n    stage clean:\n        a = copy(raw)\n    b = copy(a)\n";
    assert!(diagnose(text, None).is_empty());
}

#[test]
fn a_library_may_group_operations_in_stages() {
    let text = "stage tools:\n    operation copy(a: A) -> A\n";
    assert!(diagnose(text, None).is_empty());
}

#[test]
fn verified_files_name_what_was_checked() {
    let verified = spit::VerifiedFiles { sources: 39 }.to_string();
    assert_eq!(verified, "39 source files verified.");
}

#[test]
fn every_job_follows_the_jobs_it_depends_on() {
    // A backend may run a `.spitdag`'s jobs in the order it lists them.
    for (pipeline, sources) in [
        (PIPELINE, SOURCES),
        (NESTED, NESTED_SOURCES),
        (
            "examples/commands/mrtrix3_act/mrtrix3_act.spit",
            "examples/commands/mrtrix3_act/mrtrix3_mock_data/inputs.spitout",
        ),
    ] {
        let text = fs::read_to_string(pipeline).unwrap() + &records(sources);
        let (parsed, inventory) = support::parse_fixture(&text).unwrap();
        let dag = resolve(&parsed, &inventory.unwrap()).unwrap();
        assert!(dag.jobs.len() > 1, "{pipeline}");
        for (index, job) in dag.jobs.iter().enumerate() {
            assert_eq!(job.id.number(), index + 1, "{pipeline}");
            assert!(
                job.dependencies
                    .iter()
                    .all(|&dependency| dependency < job.id),
                "{pipeline}: job {} depends on {:?}",
                job.id,
                job.dependencies
            );
        }
    }
}
