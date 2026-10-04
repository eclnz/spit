//! Operations carried out by a body of steps: a call becomes the body's
//! steps over the caller's products, as if the caller had written them.

mod support;

use spit::{diagnose, resolve, Diagnostic};

/// Lines of reads per group and lane, a calibration table per group and
/// revision, and three operations a body can call.
const BASE: &str = "\
path: out/[{@stage}/]{@product}/{@entities}.txt
source raw : Lines [group, lane]
path raw: in/{group}/r{lane}.txt
source cal : Table [group, revision]
path cal: cal/{group}-r{revision}.txt

operation clean(x: Lines, t: Table) -> Lines
command clean: clean {x} {t} {@output}
operation merge(xs: many Lines) -> Lines
command merge: merge {xs} {@output}
operation count(x: Lines) -> Count
command count: wc {x} {@output}
";

const RECORDS: &str = "
sources:
    raw[group=a,lane=1]
    raw[group=a,lane=2]
    raw[group=b,lane=1]
    raw[group=b,lane=2]
    cal[group=a,revision=1]
    cal[group=a,revision=2]
    cal[group=b,revision=1]
    cal[group=b,revision=2]
";

const SUMMARISE: &str = "
# Clean every lane, merge them, and count the result.
operation summarise(reads: Lines, table: Table) -> (merged: Lines, total: Count):
    cleaned = clean(reads, table)
    merged = merge(cleaned @ vary(lane))
    total = count(merged)
";

/// Every job output of `pipeline`, resolved over the records.
fn outputs(pipeline: &str) -> Vec<String> {
    let (pipeline, records) = support::parse_fixture(&format!("{pipeline}{RECORDS}")).unwrap();
    support::outputs(&resolve(&pipeline, &records.unwrap()).unwrap())
}

/// Each error `check` reports on `pipeline`, as `line: message`.
fn errors(pipeline: &str) -> Vec<String> {
    support::errors(diagnose(pipeline, None))
        .iter()
        .map(|error: &Diagnostic| format!("{}: {}", error.line.unwrap_or_default(), error.message))
        .collect()
}

#[test]
fn a_call_becomes_its_bodys_steps_over_the_callers_products() {
    let text =
        format!("{BASE}{SUMMARISE}merged, total = summarise(raw, cal @ where(revision=2))\n");
    let (pipeline, _) = support::parse_fixture(&text).unwrap();
    let steps: Vec<_> = pipeline
        .invocations
        .iter()
        .map(|step| (step.operation.as_str(), step.outputs.join(",")))
        .collect();
    assert_eq!(
        steps,
        [
            ("clean", "merged::cleaned".to_owned()),
            ("merge", "merged".to_owned()),
            ("count", "total".to_owned()),
        ]
    );
    // The call is recorded, and each step names it.
    assert_eq!(pipeline.calls.len(), 1);
    assert_eq!(pipeline.calls[0].operation, "summarise");
    assert_eq!(pipeline.calls[0].instance, "merged");
    assert!(pipeline.invocations.iter().all(|step| step
        .origin
        .as_ref()
        .is_some_and(|origin| origin.call.index() == 0)));
    // The caller's `where` and the body's `vary` both hold.
    let outputs = outputs(&text);
    assert_eq!(
        outputs,
        [
            "merged::cleaned[group=a,lane=1]",
            "merged::cleaned[group=a,lane=2]",
            "merged::cleaned[group=b,lane=1]",
            "merged::cleaned[group=b,lane=2]",
            "merged[group=a]",
            "merged[group=b]",
            "total[group=a]",
            "total[group=b]",
        ]
    );
}

#[test]
fn two_calls_file_their_own_products_apart() {
    let text = format!(
        "{BASE}{SUMMARISE}first, first_total = summarise(raw, cal @ where(revision=1))\n\
         second, second_total = summarise(raw, cal @ where(revision=2))\n"
    );
    let (pipeline, records) = support::parse_fixture(&format!("{text}{RECORDS}")).unwrap();
    let dag = resolve(&pipeline, &records.unwrap()).unwrap();
    assert_eq!(dag.jobs.len(), 16);
    let paths = support::bound(&pipeline, &dag).unwrap();
    assert!(
        paths.contains("out/first.cleaned/group=a__lane=1.txt"),
        "{paths}"
    );
    assert!(
        paths.contains("out/second.cleaned/group=a__lane=1.txt"),
        "{paths}"
    );
    assert!(paths.contains("cal/a-r1.txt") && paths.contains("cal/a-r2.txt"));
}

#[test]
fn a_call_in_a_body_is_expanded_in_turn_and_in_the_callers_stage() {
    let text = format!(
        "{BASE}
operation tidy(reads: Lines, table: Table) -> (tidied: Lines):
    tidied = clean(reads, table)

operation summarise(reads: Lines, table: Table) -> (merged: Lines, total: Count):
    cleaned = tidy(reads, table)
    merged = merge(cleaned @ vary(lane))
    total = count(merged)

stage report:
    m, t = summarise(raw, cal @ where(revision=2))
"
    );
    let (pipeline, _) = support::parse_fixture(&text).unwrap();
    let products: Vec<_> = pipeline
        .invocations
        .iter()
        .map(|step| step.output_product())
        .collect();
    assert_eq!(products, ["m::cleaned", "m", "t"]);
    assert!(pipeline
        .invocations
        .iter()
        .all(|step| step.stage.as_deref() == Some("report")));
    // The inner call is the outer call's child.
    assert_eq!(pipeline.calls.len(), 2);
    assert_eq!(pipeline.calls[1].operation, "tidy");
    assert_eq!(pipeline.calls[1].instance, "m::cleaned");
    assert_eq!(pipeline.calls[1].parent.map(|call| call.index()), Some(0));
    assert_eq!(outputs(&text).len(), 8);
}

#[test]
fn a_body_is_checked_where_it_is_declared() {
    let call = "y = s(raw, cal)\n";
    for (body, expected) in [
        (
            "operation s(r: Lines, t: Table) -> Lines:\n    o = clean(r, t)\n",
            "13: operation `s` has a body, so it names each output its steps assign, as in `-> (result: Type)`",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines):\n    o = clean(q, t)\n",
            "14: the body of `s` reads `q`, which is neither one of its inputs nor made by an earlier step of it",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines, p: Lines):\n    o = clean(r, t)\n",
            "13: no step in the body of `s` makes its output `p`",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines):\n    o = later(r)\noperation later(x: Lines) -> Lines\n",
            "14: operation `later` must be declared before `s`, whose body calls it",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines):\n    o = clean(r, t)\n    o = clean(r, t)\n",
            "15: the body of `s` already has `o`; name each product once",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines .csv):\n    o = clean(r, t)\n",
            "13: output `o` of `s` takes its extension, folder and place from the step that writes it; write only its name and type",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines):\n    source x : T\n",
            "14: the body of operation `s` holds only steps, each `output = operation(inputs)`; declare operations, checks and paths outside it",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines):\n    o = clean(r, t)\n     p = clean(r, t)\n",
            "15: this step is indented differently from the other steps of operation `s`",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines):\n",
            "13: operation `s` ends its header with `:` but has no steps; indent each beneath it as `output = operation(inputs)`, or drop the `:` and give it a `command`",
        ),
        (
            "operation s(r: Lines, t: Table) -> (o: Lines):\n    o = clean(r, t)\ncommand s: run {r}\n",
            "15: operation `s` is carried out by the steps in its body, so it takes no `command` line; give each step's operation its own",
        ),
    ] {
        // One error each: the operation's call does not repeat it.
        assert_eq!(errors(&format!("{BASE}{body}{call}")), [expected], "{body}");
    }
}

#[test]
fn a_call_is_checked_at_the_call() {
    for (call, expected) in [
        (
            "y, z = summarise(raw)\n",
            "19: operation `summarise` takes 2 inputs, but this call gives 1",
        ),
        (
            "y = summarise(raw, cal)\n",
            "19: operation `summarise` takes 2 outputs, but this call gives 1",
        ),
        // Only the body's outputs are the caller's to read.
        (
            "y, z = summarise(raw, cal @ where(revision=1))\nw = count(y::cleaned)\n",
            "20: `y::cleaned` is made inside the call `y = summarise(...)` on line 19; make it an output of `summarise` to read it here",
        ),
        // A type the body's steps do not accept is reported at the call.
        (
            "y, z = summarise(cal, cal @ where(revision=1))\n",
            "19: type mismatch at `clean.x`: product `cal` is Table, expected Lines",
        ),
    ] {
        assert_eq!(errors(&format!("{BASE}{SUMMARISE}{call}")), [expected], "{call}");
    }
}

#[test]
fn a_declared_output_type_holds_the_product_the_body_makes() {
    let text = format!(
        "{BASE}
operation wrap(x: Lines, t: Table) -> (y: Count):
    y = clean(x, t)
out = wrap(raw, cal @ where(revision=1))
"
    );
    assert_eq!(
        errors(&text),
        ["16: type mismatch at `clean.output`: product `out` is Count, expected Lines"]
    );
}

#[test]
fn an_operation_with_a_body_needs_no_command_and_its_call_uses_it() {
    let text =
        format!("{BASE}{SUMMARISE}merged, total = summarise(raw, cal @ where(revision=2))\n");
    let warnings: Vec<_> = diagnose(&text, None)
        .into_iter()
        .map(|warning| warning.message)
        .collect();
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A library that imports another, with an operation whose body calls an
/// operation of each, and checks on its ports and outputs.
fn library(tree: &support::Tree) {
    tree.write(
        "libs/sub/count.spit",
        "operation count(x: Lines) -> Count\ncommand count: wc {x} {@output}\n",
    );
    tree.write(
        "libs/lib.spit",
        "\
use sub/count.spit as C
check nonempty: test -s {@path}
check lines(n): count_lines {@path} {n}
operation clean(x: Lines, t: Table) -> Lines @ check(nonempty)
command clean: clean {x} {t} {@output}
operation merge(xs: many Lines) -> Lines
command merge: merge {xs} {@output}

operation tidy(r: Lines @ check(lines(1)), t: Table) -> (o: Lines @ check(nonempty)):
    o = clean(r, t)

operation summarise(reads: Lines @ check(lines(2)), table: Table) -> (merged: Lines @ check(lines(9)), total: Count):
    cleaned = tidy(reads, table)
    merged = merge(cleaned @ vary(lane))
    total = C::count(merged)
",
    );
}

#[test]
fn an_imported_operation_brings_the_operations_its_body_calls() {
    let tree = support::Tree::new("composite-import", &[]);
    library(&tree);
    let text = "\
use summarise from libs/lib.spit as L
source raw : Lines [group, lane]
source cal : Table [group, revision]
m, t = L::summarise(raw, cal @ where(revision=2))
";
    let main = tree.write("main.spit", text);
    let (pipeline, _) = support::parse_fixture_at(text, &main).unwrap();
    let operations: Vec<_> = pipeline
        .operations
        .iter()
        .map(|operation| (operation.name.as_str(), operation.file.as_deref()))
        .collect();
    assert_eq!(
        operations,
        [
            ("L::summarise", Some("libs/lib.spit")),
            // What its body calls, in the order the library declares it.
            ("L::C::count", Some("libs/sub/count.spit")),
            ("L::clean", Some("libs/lib.spit")),
            ("L::merge", Some("libs/lib.spit")),
            ("L::tidy", Some("libs/lib.spit")),
        ]
    );
    let steps: Vec<_> = pipeline
        .invocations
        .iter()
        .map(|step| step.operation.as_str())
        .collect();
    assert_eq!(steps, ["L::clean", "L::merge", "L::C::count"]);
}

#[test]
fn checks_on_a_bodys_ports_and_outputs_run_on_the_steps_that_read_and_make_them() {
    let tree = support::Tree::new("composite-checks", &[]);
    library(&tree);
    let text = format!(
        "\
use summarise from libs/lib.spit as L
path: out/{{@product}}/{{@entities}}.txt
source raw : Lines [group, lane]
path raw: in/{{group}}/r{{lane}}.txt
source cal : Table [group, revision]
path cal: cal/{{group}}-r{{revision}}.txt
m, t = L::summarise(raw, cal @ where(revision=2))
{RECORDS}"
    );
    let main = tree.write("main.spit", &text);
    let (pipeline, records) = support::parse_fixture_at(&text, &main).unwrap();
    let dag = resolve(&pipeline, &records.unwrap()).unwrap();
    let bound = spit::bind_dag(&pipeline, &dag).unwrap();
    let commands = spit::render_bound_dag(
        &bound,
        spit::View {
            commands: true,
            ..spit::View::default()
        },
    );
    let job = |number: &str| {
        let start = commands.find(&format!("Job {number} ")).unwrap();
        let end = commands[start..]
            .find("\n\n")
            .map_or(commands.len(), |end| start + end);
        commands[start..end].to_owned()
    };
    // Both bodies' input checks run before `clean`, and its own `nonempty`
    // and the inner body's, the same check, run once after it.
    assert_eq!(
        job("1"),
        "Job 1  L::clean\n  \
         check:  count_lines in/a/r1.txt 1\n  \
         check:  count_lines in/a/r1.txt 2\n  \
         run:    clean in/a/r1.txt cal/a-r2.txt out/m.cleaned/group=a__lane=1.txt\n  \
         check:  test -s out/m.cleaned/group=a__lane=1.txt"
    );
    // The outer body's output check runs after the step that makes it.
    assert!(
        job("5").ends_with("  check:  count_lines out/m/group=a.txt 9"),
        "{}",
        job("5")
    );
}
