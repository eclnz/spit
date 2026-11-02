//! Reviews: a person's verdicts on the artifacts a pipeline attaches a
//! `review` to, which the plan applies by withholding what a verdict fails
//! or nobody has judged.

mod support;

use spit::{
    diagnose, parse_pipeline, parse_source_inventory, render_source_inventory, resolve,
    resolve_artifacts_excluding, ArtifactReport, InputSpec, Judged,
};
use support::{check_output, outputs, spit, text, Tree};

/// Subjects with a T1 image each, summarised one by one and tallied.
const PIPELINE: &str = "\
review looked_at
source t1w : T1w [sub] @review(looked_at)
operation summarise(t1: T1w) -> Sum
operation tally(items: many Sum) -> Tally
sums = summarise(t1w)
total = tally(sums @vary(sub))
";

/// The sources of subjects 01 to 04, and the verdicts under `verdicts:`.
fn inventory(verdicts: &str) -> String {
    format!(
        "sources:\n    t1w[sub=01]\n    t1w[sub=02]\n    t1w[sub=03]\n    t1w[sub=04]\n{verdicts}"
    )
}

fn report(pipeline: &str, records: &str) -> ArtifactReport {
    let pipeline = parse_pipeline(pipeline).unwrap();
    let inventory = parse_source_inventory(records).unwrap();
    resolve_artifacts_excluding(&pipeline, &inventory, &[]).unwrap()
}

/// Every verdict row for review `looked_at`: `sub=..` and its verdict.
fn verdicts(rows: &[(&str, &str)]) -> String {
    let mut text = String::from("\nverdicts:\n");
    for (index, (sub, verdict)) in rows.iter().enumerate() {
        text += &format!(
            "    looked_at[sub={sub}]\n        verdict: {verdict}\n        at: qc/looked_at.csv line {}\n",
            index + 2
        );
    }
    text
}

fn notes(report: &ArtifactReport) -> Vec<String> {
    report.withheld.iter().map(ToString::to_string).collect()
}

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| format!("{}: {}", diagnostic.line.unwrap_or(0), diagnostic.message))
        .collect()
}

#[test]
fn a_review_takes_no_command_and_no_parameters() {
    assert_eq!(
        errors("review looked_at: look {@path}\n"),
        ["1: a review takes no command, because a person judges it; write `review looked_at` in the pipeline, and name its verdicts in the recipe with `review looked_at from file.csv`"]
    );
    assert!(errors("review looked_at(3)\n")[0].contains("takes no parameters"));
    assert!(errors("review\n")[0].contains("expected `review name`"));
    assert!(errors("review looked_at\nreview looked_at\n")[0].contains("duplicate review"));
}

#[test]
fn reviews_are_declared_in_a_block_and_attached_where_checks_are() {
    let pipeline = "\
review:
    looked_at
    second
check nonempty: test -s {@path}
source t1w : T1w [sub] @check(nonempty) @review(looked_at, second)
operation summarise(t1: T1w @review(second)) -> Sum @review(looked_at, !second)
operation split(t1: T1w) -> (a: Sum @review(looked_at), b: Sum)
default review: second
";
    assert_eq!(errors(pipeline), Vec::<String>::new());
    let parsed = parse_pipeline(pipeline).unwrap();
    assert_eq!(parsed.reviews.len(), 2);
    assert_eq!(parsed.products[0].reviews, ["looked_at", "second"]);
    assert_eq!(parsed.operations[0].inputs[0].reviews, ["second"]);
    assert_eq!(parsed.operations[0].outputs[0].reviews, ["looked_at"]);
    assert_eq!(parsed.operations[0].outputs[0].exempt_reviews, ["second"]);
    assert_eq!(parsed.default_reviews.reviews, ["second"]);
}

#[test]
fn an_unknown_review_is_an_error_wherever_it_is_attached() {
    for (text, line) in [
        ("source t1w : T1w [sub] @review(nobody)\n", 1),
        ("operation f(x: T @review(nobody)) -> T\n", 1),
        ("operation f(x: T) -> T @review(nobody)\n", 1),
        ("default review: nobody\n", 1),
        ("stage s:\n    default review: nobody\n", 2),
    ] {
        let found = errors(text);
        assert_eq!(found.len(), 1, "{text}: {found:?}");
        assert!(
            found[0].starts_with(&format!("{line}: unknown review `nobody`")),
            "{found:?}"
        );
    }
}

#[test]
fn misplaced_reviews_are_errors() {
    assert!(
        errors("review x\noperation f(x: T) -> T\ny : T [a] @review(x) = f(raw)\n")[0]
            .contains("a step's product takes no `@check(...)` or `@review(...)`")
    );
    assert!(errors("review x\noperation f(x: T) @review(x)\n")[0]
        .contains("`@review(...)` follows the port it applies to"));
    assert!(errors("review x\nsource t [a] @review(!x)\n")[0].contains("cannot opt out"));
    assert!(errors("review x\nsource t [a] @review(x(1))\n")[0].contains("takes no arguments"));
    assert!(errors("review x from f.csv\n")[0].contains("belong in a .spitin recipe"));
}

#[test]
fn a_passed_artifact_is_used_and_a_failed_or_unjudged_one_is_withheld() {
    let report = report(
        PIPELINE,
        &inventory(&verdicts(&[("01", "pass"), ("02", "fail")])),
    );
    // Only subject 01 passed, so the other three are kept from every reader.
    assert_eq!(outputs(&report.dag), ["sums[sub=01]", "total"]);
    assert!(report.incomplete.is_empty());
    assert_eq!(
        notes(&report),
        [
            "review `looked_at` rejected t1w[sub=02] (qc/looked_at.csv line 3)",
            "review `looked_at` holds back t1w[sub=03]: no verdict yet",
            "review `looked_at` holds back t1w[sub=04]: no verdict yet",
        ]
    );
}

#[test]
fn withholding_is_no_error() {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let inventory = parse_source_inventory(&inventory("")).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    // Nothing is judged, so no summary is made and no aggregate has members.
    assert!(dag.jobs.is_empty());
}

#[test]
fn a_one_input_leaves_the_job_out_and_withholds_its_outputs_in_turn() {
    let pipeline = "\
review looked_at
source t1w : T1w [sub] @review(looked_at)
operation summarise(t1: T1w) -> Sum
operation render(s: Sum) -> Picture
sums = summarise(t1w)
pictures = render(sums)
";
    let report = report(
        pipeline,
        &inventory(&verdicts(&[("01", "pass"), ("02", "fail")])),
    );
    assert_eq!(outputs(&report.dag), ["sums[sub=01]", "pictures[sub=01]"]);
    assert!(report.incomplete.is_empty());
}

#[test]
fn an_aggregate_takes_the_members_that_remain() {
    let report = report(
        PIPELINE,
        &inventory(&verdicts(&[
            ("01", "pass"),
            ("02", "fail"),
            ("03", "pass"),
            ("04", "pass"),
        ])),
    );
    assert_eq!(
        outputs(&report.dag),
        ["sums[sub=01]", "sums[sub=03]", "sums[sub=04]", "total"]
    );
    let tally = report.dag.jobs.last().unwrap();
    assert_eq!(tally.inputs[0].len(), 3);
}

#[test]
fn a_review_on_an_output_withholds_what_the_job_made_but_still_runs_it() {
    let pipeline = "\
review looked_at
source t1w : T1w [sub]
operation summarise(t1: T1w) -> Sum @review(looked_at)
operation tally(items: many Sum) -> Tally
sums = summarise(t1w)
total = tally(sums @vary(sub))
";
    let report = report(
        pipeline,
        &inventory(&verdicts(&[("01", "pass"), ("02", "fail")])),
    );
    assert_eq!(
        outputs(&report.dag),
        [
            "sums[sub=01]",
            "sums[sub=02]",
            "sums[sub=03]",
            "sums[sub=04]",
            "total"
        ]
    );
    assert_eq!(report.dag.jobs.last().unwrap().inputs[0].len(), 1);
    assert_eq!(
        notes(&report)[0],
        "review `looked_at` rejected sums[sub=02] (qc/looked_at.csv line 3)"
    );
}

#[test]
fn a_review_on_an_input_port_covers_that_port_alone() {
    let pipeline = "\
review looked_at
source t1w : T1w [sub]
operation reviewed(t1: T1w @review(looked_at)) -> Sum
operation plain(t1: T1w) -> Raw
sums = reviewed(t1w)
raws = plain(t1w)
";
    let report = report(pipeline, &inventory(&verdicts(&[("01", "pass")])));
    assert_eq!(
        outputs(&report.dag),
        [
            "sums[sub=01]",
            "raws[sub=01]",
            "raws[sub=02]",
            "raws[sub=03]",
            "raws[sub=04]"
        ]
    );
    assert_eq!(
        notes(&report)[0],
        "review `looked_at` holds back t1w[sub=02] at `t1` of `reviewed`: no verdict yet"
    );
    // The same artifact is noted once, whatever reads it.
    assert_eq!(report.withheld.len(), 3);
}

#[test]
fn a_default_review_judges_outputs_and_an_output_can_opt_out() {
    let pipeline = "\
review looked_at
source t1w : T1w [sub]
default review: looked_at
operation summarise(t1: T1w) -> Sum
operation measure(t1: T1w) -> Number @review(!looked_at)
sums = summarise(t1w)
numbers = measure(t1w)
";
    let report = report(pipeline, &inventory(&verdicts(&[("01", "fail")])));
    assert_eq!(report.withheld.len(), 4);
    assert!(report
        .withheld
        .iter()
        .all(|withheld| withheld.artifact.product == "sums"));
}

#[test]
fn a_row_with_empty_cells_covers_a_whole_group() {
    let pipeline = "\
review looked_at
source image : Image [sub, ses] @review(looked_at)
operation summarise(i: Image) -> Sum
sums = summarise(image)
";
    let records = "\
sources:
    image[sub=01,ses=1]
    image[sub=01,ses=2]
    image[sub=02,ses=1]

verdicts:
    looked_at[sub=01]
        verdict: fail
        at: qc/x.csv line 2
    looked_at[sub=02,ses=1]
        verdict: pass
        at: qc/x.csv line 3
";
    let report = report(pipeline, records);
    assert_eq!(outputs(&report.dag), ["sums[ses=1,sub=02]"]);
    assert_eq!(report.withheld.len(), 2);
    assert!(matches!(
        report.withheld[0].outcome,
        Judged::Rejected { .. }
    ));
}

#[test]
fn a_row_that_matches_no_artifact_is_noted_once_per_file() {
    let report = report(
        PIPELINE,
        &inventory(&verdicts(&[
            ("01", "pass"),
            ("08", "fail"),
            ("09", "pass"),
            ("11", "pass"),
        ])),
    );
    assert_eq!(report.unmatched_verdicts.len(), 1);
    assert_eq!(
        report.unmatched_verdicts[0].to_string(),
        "3 verdicts in qc/looked_at.csv match no artifact (lines 3\u{2013}5)"
    );
    let report = self::report(
        PIPELINE,
        &inventory(&verdicts(&[("01", "pass"), ("08", "pass")])),
    );
    assert_eq!(
        report.unmatched_verdicts[0].to_string(),
        "1 verdict in qc/looked_at.csv matches no artifact (line 3)"
    );
}

#[test]
fn the_spitout_writes_the_verdicts_and_reads_them_back() {
    let records = inventory(&verdicts(&[("01", "pass"), ("02", "fail")]));
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let mut read = parse_source_inventory(&records).unwrap();
    assert_eq!(read.verdicts.len(), 2);
    read.verdicts[1].reason = Some("ghosting # frontal".to_owned());
    let written = render_source_inventory(&read, &pipeline, &InputSpec::default().rules);
    assert!(written.contains(
        "verdicts:\n    looked_at[sub=01]\n        verdict: pass\n        at: qc/looked_at.csv line 2\n"
    ));
    let again = parse_source_inventory(&written).unwrap();
    assert_eq!(again.verdicts, read.verdicts);
    assert_eq!(
        render_source_inventory(&again, &pipeline, &InputSpec::default().rules),
        written
    );
    for (bad, message) in [
        (
            "verdicts:\n    looked_at[sub=1]\n",
            "needs a `verdict: pass`",
        ),
        ("verdicts:\n        verdict: pass\n", "needs the review"),
        (
            "verdicts:\n    looked_at[sub=1]\n        verdict: maybe\n",
            "`pass` or `fail`",
        ),
        (
            "verdicts:\n    looked_at[sub=1]\n        at: qc.csv\n",
            "a file and a line",
        ),
    ] {
        let error = parse_source_inventory(bad).unwrap_err().to_string();
        assert!(error.contains(message), "{bad}: {error}");
    }
}

/// A dataset of four subjects and a recipe naming a review file.
fn dataset(name: &str, recipe: &str) -> Tree {
    let tree = Tree::new(
        name,
        &[
            "sub-01/t1w.txt",
            "sub-02/t1w.txt",
            "sub-03/t1w.txt",
            "sub-04/t1w.txt",
        ],
    );
    tree.write(
        "pipeline.spit",
        &format!(
            "{PIPELINE}path t1w: sub-{{sub}}/t1w.txt\npath sums: out/sum-{{sub}}.txt\npath total: out/total.txt\n"
        ),
    );
    tree.write(
        "data.spitin",
        &format!("pipeline pipeline.spit\nroot .\n{recipe}"),
    );
    tree
}

fn run(tree: &Tree, command: &str) -> (bool, String, String) {
    let recipe = tree.path().join("data.spitin");
    let output = spit(&[command, recipe.to_str().unwrap()]);
    (
        output.status.success(),
        text(&output.stdout),
        text(&output.stderr),
    )
}

#[test]
fn a_review_file_gives_the_verdicts_and_the_commands_note_what_they_did() {
    let tree = dataset("review-file", "review looked_at from qc/looked_at.csv\n");
    tree.write(
        "qc/looked_at.csv",
        "sub,verdict,reason\n01,pass,\n02,fail,\"ghosting, frontal\"\n08,pass,\n09,pass,\n",
    );
    let (ok, stdout, stderr) = run(&tree, "dag");
    assert!(ok, "{stderr}");
    assert!(stdout.contains("Job 2"), "{stdout}");
    assert!(
        stderr.contains("note: review `looked_at` rejected t1w[sub=02] (qc/looked_at.csv line 3): ghosting, frontal"),
        "{stderr}"
    );
    assert!(
        stderr.contains("note: review `looked_at` holds back t1w[sub=03]: no verdict yet"),
        "{stderr}"
    );
    assert!(
        stderr
            .contains("note: 2 verdicts in qc/looked_at.csv match no artifact (lines 4\u{2013}5)"),
        "{stderr}"
    );
    let (ok, _, stderr) = run(&tree, "artifacts");
    assert!(ok, "{stderr}");
    assert!(stderr.contains("rejected t1w[sub=02]"), "{stderr}");
}

#[test]
fn a_review_without_a_file_holds_back_everything_it_judges() {
    for recipe in ["", "review looked_at from qc/looked_at.csv\n"] {
        let tree = dataset("review-missing", recipe);
        let (ok, stdout, stderr) = run(&tree, "dag");
        assert!(ok, "{stderr}");
        assert!(!stdout.contains("Job 1"), "{stdout}");
        assert!(stderr.contains("holds back t1w[sub=01]"), "{stderr}");
        assert!(!stderr.contains("resolves no jobs"), "{stderr}");
        if !recipe.is_empty() {
            let (_, _, stderr) = run(&tree, "inputs");
            assert!(
                stderr.contains(
                    "review `looked_at` has no verdicts yet: `qc/looked_at.csv` does not exist"
                ),
                "{stderr}"
            );
        }
    }
}

#[test]
fn the_verdicts_reach_the_spitout_and_a_dag_made_from_it_agrees() {
    let tree = dataset("review-spitout", "review looked_at from qc/looked_at.csv\n");
    tree.write("qc/looked_at.csv", "sub,verdict\n01,pass\n02,fail\n");
    let recipe = tree.path().join("data.spitin");
    let spitout = tree.path().join("data.spitout");
    let output = spit(&[
        "inputs",
        recipe.to_str().unwrap(),
        "-o",
        spitout.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let written = std::fs::read_to_string(&spitout).unwrap();
    assert!(
        written.contains("verdicts:\n    looked_at[sub=01]\n        verdict: pass\n        at: qc/looked_at.csv line 2\n"),
        "{written}"
    );
    check_output("reviews_spitout", &written);
    let pipeline = tree.path().join("pipeline.spit");
    let from_spitout = spit(&["dag", pipeline.to_str().unwrap(), spitout.to_str().unwrap()]);
    let from_recipe = spit(&["dag", recipe.to_str().unwrap()]);
    assert!(
        from_spitout.status.success(),
        "{}",
        text(&from_spitout.stderr)
    );
    assert_eq!(text(&from_spitout.stdout), text(&from_recipe.stdout));
    assert!(text(&from_spitout.stderr).contains("rejected t1w[sub=02]"));
}

#[test]
fn a_malformed_review_file_is_an_error_that_names_its_file_and_line() {
    let tree = dataset("review-errors", "review looked_at from qc/looked_at.csv\n");
    for (csv, message) in [
        (
            "sub,verdict\n01,maybe\n",
            "qc/looked_at.csv line 2: verdict `maybe` must be `pass` or `fail`",
        ),
        (
            "sub,ses,verdict\n01,1,pass\n",
            "qc/looked_at.csv line 1: column `ses` is not a dimension of `t1w`",
        ),
        (
            "sub,verdict\n,pass\n",
            "qc/looked_at.csv line 2: names no dimension value",
        ),
        (
            "sub,reason\n01,x\n",
            "qc/looked_at.csv line 1: expected a `verdict` column",
        ),
        (
            "sub,verdict\n01,pass\n01,fail\n",
            "qc/looked_at.csv line 3: verdict `fail` conflicts with `pass` on line 2",
        ),
        (
            "sub,verdict\n01,pass\n\"02,fail\n",
            "qc/looked_at.csv line 3: a quoted field is not closed",
        ),
    ] {
        tree.write("qc/looked_at.csv", csv);
        for command in ["inputs", "dag"] {
            let (ok, _, stderr) = run(&tree, command);
            assert!(!ok, "{csv}");
            assert!(stderr.contains(message), "{command} {csv}: {stderr}");
        }
    }
}

#[test]
fn a_recipe_names_only_reviews_the_pipeline_declares() {
    let tree = dataset("review-unknown", "review nobody from qc/nobody.csv\n");
    let (ok, _, stderr) = run(&tree, "check");
    assert!(!ok);
    assert!(
        stderr.contains("the pipeline declares no review `nobody`"),
        "{stderr}"
    );
    let tree = dataset(
        "review-twice",
        "review looked_at from a.csv\nreview looked_at from b.csv\n",
    );
    let (ok, _, stderr) = run(&tree, "check");
    assert!(!ok);
    assert!(
        stderr.contains("already has its file `a.csv` on line 3"),
        "{stderr}"
    );
}

#[test]
fn exclude_from_is_not_a_recipe_rule_and_says_what_to_write() {
    let tree = dataset("exclude-from", "exclude from qc/excluded.csv\n");
    for command in ["check", "inputs", "dag"] {
        let (ok, _, stderr) = run(&tree, command);
        assert!(!ok, "{command}");
        assert!(
            stderr.contains("`exclude from` is not a recipe rule; to leave out what a person judged, declare a `review` in the pipeline, attach it with `@review(name)`, and write `review name from file.csv` in the recipe"),
            "{command}: {stderr}"
        );
    }
}

#[test]
fn a_review_is_declared_in_a_pipeline_and_named_in_a_recipe() {
    let tree = dataset("review-misplaced", "review looked_at\n");
    let (ok, _, stderr) = run(&tree, "check");
    assert!(!ok);
    assert!(
        stderr.contains("a recipe names the file of a review"),
        "{stderr}"
    );
}

#[test]
fn an_import_brings_the_reviews_its_operations_and_sources_attach() {
    let tree = Tree::new("review-import", &[]);
    tree.write(
        "lib.spit",
        "review looked_at\nsource ref : T [sub] @review(looked_at)\noperation f(x: T @review(looked_at)) -> T @review(looked_at)\n",
    );
    tree.write(
        "a.spit",
        "use lib.spit\ny = f(ref)\npath y: out/{sub}.txt\npath ref: in/{sub}.txt\n",
    );
    tree.write("b.spit", "use lib.spit as lib\ny = lib::f(lib::ref)\npath y: out/{sub}.txt\npath lib::ref: in/{sub}.txt\n");
    tree.write(
        "c.spit",
        "review looked_at\nuse f from lib.spit\nsource raw [sub]\ny = f(raw)\npath y: out/{sub}.txt\npath raw: in/{sub}.txt\n",
    );
    for file in ["a.spit", "b.spit", "c.spit"] {
        let path = tree.path().join(file);
        let output = spit(&["check", path.to_str().unwrap()]);
        assert!(output.status.success(), "{file}: {}", text(&output.stderr));
    }
    let pipeline = spit::parse_pipeline_at(
        &std::fs::read_to_string(tree.path().join("b.spit")).unwrap(),
        &tree.path().join("b.spit"),
    )
    .unwrap();
    assert_eq!(pipeline.reviews[0].name, "lib::looked_at");
    assert_eq!(pipeline.operations[0].inputs[0].reviews, ["lib::looked_at"]);
    assert_eq!(pipeline.products[0].reviews, ["lib::looked_at"]);
}
