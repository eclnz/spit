mod support;

use spit::{
    diagnose, parse_pipeline, parse_source_inventory, render_source_inventory, resolve, InputSpec,
};
use support::{check_output, spit, text, Tree};

const PIPELINE: &str = "\
review looked_at
source t1w : T1w [sub] @review(looked_at)
operation summarise(t1: T1w) -> Sum
operation tally(items: many Sum) -> Tally
sums = summarise(t1w)
total = tally(sums @vary(sub))
";

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
fn a_review_plans_every_job_whatever_its_verdicts() {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let records = "sources:\n    t1w[sub=01]\n    t1w[sub=02]\n\nverdicts:\n    looked_at from qc/looked_at.csv\n";
    let inventory = parse_source_inventory(records).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 3);
}

#[test]
fn the_spitout_records_only_where_each_review_file_is() {
    let records = "sources:\n    t1w[sub=01]\n\nverdicts:\n    looked_at from qc/looked_at.csv\n";
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let read = parse_source_inventory(records).unwrap();
    assert_eq!(read.review_files.len(), 1);
    let written = render_source_inventory(&read, &pipeline, &InputSpec::default().rules);
    assert!(
        written.ends_with("verdicts:\n    looked_at from qc/looked_at.csv\n"),
        "{written}"
    );
    assert_eq!(parse_source_inventory(&written).unwrap(), read);
    for bad in [
        "verdicts:\n    looked_at[sub=1]\n        verdict: pass\n",
        "verdicts:\n    looked at from qc.csv\n",
        "verdicts:\n    looked_at from \n",
    ] {
        let error = parse_source_inventory(bad).unwrap_err().to_string();
        assert!(error.contains("review"), "{bad}: {error}");
    }
}

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
fn potential_dag_carries_review_scope_and_ignores_the_sheet_contents() {
    let tree = dataset("review-file", "review looked_at from qc/looked_at.csv\n");
    let recipe = tree.path().join("data.spitin");
    let build = || {
        let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
        assert!(output.status.success(), "{}", text(&output.stderr));
        text(&output.stdout)
    };
    let before = build();
    tree.write("qc/looked_at.csv", "sub,verdict\n01,fail\n02,INVALID\n");
    let after = build();
    assert_eq!(before, after);
    let checked = spit(&["check", recipe.to_str().unwrap(), "--path-rules"]);
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(after.contains("\"dimensions\":[\"sub\"],\"targets\""));
    assert!(after.contains("\"input\":null"));
    assert!(after.contains("\"id\":5"));
    let (ok, _, stderr) = run(&tree, "artifacts");
    assert!(ok, "{stderr}");
    assert!(!stderr.contains("rejected t1w"), "{stderr}");
}

#[test]
fn an_inventory_round_trip_preserves_the_review_file_reference() {
    let tree = dataset("review-spitout", "review looked_at from qc/looked_at.csv\n");
    tree.write("qc/looked_at.csv", "sub,verdict\n01,pass\n02,fail\n");
    let recipe = tree.path().join("data.spitin");
    let spitout = tree.path().join("records/data.spitout");
    std::fs::create_dir_all(spitout.parent().unwrap()).unwrap();
    let output = spit(&[
        "inputs",
        recipe.to_str().unwrap(),
        "-o",
        spitout.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let written = std::fs::read_to_string(&spitout).unwrap();
    assert!(
        written.contains("verdicts:\n    looked_at from ../qc/looked_at.csv\n"),
        "{written}"
    );
    check_output("reviews_spitout", &written);
    let pipeline = tree.path().join("pipeline.spit");
    let from_spitout = spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        spitout.to_str().unwrap(),
        "--json",
    ]);
    let from_recipe = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(
        from_spitout.status.success(),
        "{}",
        text(&from_spitout.stderr)
    );
    assert!(
        from_recipe.status.success(),
        "{}",
        text(&from_recipe.stderr)
    );
    assert_eq!(
        text(&from_spitout.stdout).replace("/records/..", ""),
        text(&from_recipe.stdout)
    );
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
