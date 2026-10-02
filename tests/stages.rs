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
fn stages_must_not_depend_on_each_other_in_a_cycle() {
    // `glue` sits outside every stage, so `late` reads from `second` through it.
    let text = "source raw [id]\noperation copy(a: A) -> A\noperation pair(a: A, a2: A) -> A\nstage first:\n    a = copy(raw)\n    d = pair(a, late)\nstage second:\n    b = copy(a)\nglue = copy(b)\nstage third:\n    late = copy(glue)\n";
    let diagnostics = diagnose(text, None);
    assert_eq!(
        messages(&diagnostics),
        [(
            Some(4),
            "stages must not depend on each other in a cycle: `d` in `first` reads `late` from `third`, `late` in `third` reads `b` from `second`, and `b` in `second` reads `a` from `first`"
        )]
    );
    let (pipeline, _) = support::parse_fixture(text).unwrap();
    assert!(resolve(&pipeline, &Default::default()).is_err());
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
            "source raw [id]\nstage prep:\n    require raw count>=1 per [id]\n",
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
            "stage prep:\n    stage inner:\n    stage inner:\n",
            (Some(3), "duplicate stage `prep/inner`: it is already opened on line 2; a stage is one block, so move these lines into it"),
        ),
        (
            "stage prep\n",
            (
                Some(1),
                "expected `stage name:`, with the stage's lines indented beneath it",
            ),
        ),
        (
            "stage prep:\nstage prep:\n",
            (Some(2), "duplicate stage `prep`: it is already opened on line 1; a stage is one block, so move these lines into it"),
        ),
        (
            "stage prep:\n    path: a/{@product}/{@entities}\n    path: b/{@product}/{@entities}\n",
            (Some(3), "duplicate default path template for stage `prep`"),
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
fn one_stage_includes_the_stages_nested_in_it() {
    let (pipeline, inventory) = support::parse_fixture(&nested()).unwrap();
    let dag = resolve(&pipeline, &inventory.unwrap()).unwrap();
    let ids = |stage: &str| -> Vec<_> {
        dag.only_stage(stage)
            .jobs
            .iter()
            .map(|job| job.id.number())
            .collect()
    };
    assert_eq!(ids("preprocess"), [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(ids("preprocess/combine"), [4, 5]);
    assert_eq!(ids("pre"), Vec::<usize>::new());
    let (ok, _, stderr) = spit(&["dag", NESTED, NESTED_SOURCES]);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("9 jobs resolved: 7 in preprocess, 2 in analysis."));
}

#[test]
fn nested_siblings_must_not_depend_on_each_other_in_a_cycle() {
    // `glue` sits in `outer` itself, so `b` reads from `first` through it.
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage outer:\n    glue : Item [id] = copy(a)\n    stage first:\n        a = copy(raw)\n        c : Item [id] = copy(b)\n    stage second:\n        b = copy(glue)\n";
    assert_eq!(
        messages(&diagnose(text, None)),
        [(
            Some(5),
            "stages must not depend on each other in a cycle: `c` in `outer/first` reads `b` from `outer/second` and `b` in `outer/second` reads `a` from `outer/first`"
        )]
    );
}

#[test]
fn nested_stages_count_toward_their_outer_stages_cycles() {
    let text = "source raw [id]\noperation copy(a: A) -> A\nstage prep:\n    stage deep:\n        a : Item [id] = copy(late)\n        c = copy(raw)\nstage later:\n    stage deep:\n        late = copy(c)\n";
    assert_eq!(
        messages(&diagnose(text, None)),
        [(
            Some(3),
            "stages must not depend on each other in a cycle: `a` in `prep` reads `late` from `later` and `late` in `later` reads `c` from `prep`"
        )]
    );
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
fn a_library_may_group_operations_in_stages() {
    let text = "stage tools:\n    operation copy(a: A) -> A\n";
    assert!(diagnose(text, None).is_empty());
}

#[test]
fn verified_files_name_what_was_checked() {
    let verified = |sources, made_elsewhere| {
        spit::VerifiedFiles {
            sources,
            made_elsewhere,
        }
        .to_string()
    };
    assert_eq!(verified(39, 0), "39 source files verified.");
    assert_eq!(verified(0, 2), "2 files made outside the stage verified.");
    assert_eq!(
        verified(3, 2),
        "3 source files and 2 files made outside the stage verified."
    );
}

#[test]
fn every_job_follows_the_jobs_it_depends_on() {
    // A backend may run a `.spitdag`'s jobs in the order it lists them.
    for (pipeline, sources) in [
        (PIPELINE, SOURCES),
        (NESTED, NESTED_SOURCES),
        (
            "examples/commands/mrtrix3_act/mrtrix3_act.spit",
            "examples/commands/mrtrix3_act/mrtrix3_act.spitout",
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
