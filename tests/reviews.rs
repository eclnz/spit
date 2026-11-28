mod support;

use spit::{
    diagnose, parse_pipeline, parse_source_inventory, render_source_inventory, resolve, InputSpec,
};
use support::{check_output, spit, text, Tree};

const PIPELINE: &str = "\
source t1w : T1w [sub]
operation summarise(t1: T1w) -> Sum
operation tally(items: many Sum) -> Tally
sums = summarise(t1w) @review
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
fn review_syntax_is_only_a_suffix_on_a_call() {
    let prelude = "source raw : T [sub]\noperation f(x: T) -> T\npath raw: in/{sub}.txt\npath y: out/{sub}.txt\n";
    for form in [
        "review y",
        "review y from qc.csv",
        "review:\n    y",
        "default review: y",
        "source direct : T @review(y)",
        "operation input(x: T @review(y)) -> T",
        "operation output(x: T) -> T @review(y)",
        "y = f(raw @review(y))",
        "y = f(raw) @review()",
        "y = f(raw) @review @review",
        "y = f(raw) @review(raw, raw)",
        "y = f(raw) @review(product: y)",
        "y = f(raw) @review(evidence: raw)",
    ] {
        assert!(!errors(&format!("{prelude}{form}\n")).is_empty(), "{form}");
    }
}

#[test]
fn bare_review_applies_to_each_output_without_evidence() {
    let text = "\
source raw : T [sub]
operation split(x: T) -> (left: T, right: T)
a, b = split(raw) @review
default path: out/{@product}/{sub}.txt
";
    assert!(errors(text).is_empty(), "{:?}", errors(text));
    let pipeline = parse_pipeline(text).unwrap();
    let reviews = &pipeline.invocations[0].reviews;
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[0].port, spit::Port::Output(0));
    assert_eq!(reviews[1].port, spit::Port::Output(1));
    assert!(reviews
        .iter()
        .all(|review| review.name.is_none() && review.evidence.is_empty()));
}

const REVIEW_EVIDENCE: &str = "\
source raw : T [sub, run]
operation fit(raw: T) -> (image: T, metadata: T)
operation plot(images: many T) -> Plot
image, metadata = fit(raw) @review(figures, contacts)
figures = plot(image @vary(run))
contacts = plot(image @vary(run))
default path: out/{@product}/{sub}[/{run}].txt
";

#[test]
fn a_review_lists_forward_evidence_for_each_output() {
    assert_eq!(errors(REVIEW_EVIDENCE), Vec::<String>::new());
    let pipeline = parse_pipeline(REVIEW_EVIDENCE).unwrap();
    let reviews = &pipeline.invocations[0].reviews;
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[0].port, spit::Port::Output(0));
    assert_eq!(reviews[1].port, spit::Port::Output(1));
    assert!(reviews
        .iter()
        .all(|review| review.evidence == ["figures", "contacts"]));
}

#[test]
fn a_saved_dag_binds_shared_evidence_by_entities() {
    let tree = Tree::new(
        "review-evidence-dag",
        &["in/01/1.txt", "in/01/2.txt", "in/02/1.txt"],
    );
    tree.write(
        "pipeline.spit",
        &format!("{REVIEW_EVIDENCE}path raw: in/{{sub}}/{{run}}.txt\n"),
    );
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\nreview image from qc.csv\nreview metadata from qc/meta.csv\n",
    );
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let saved = text(&output.stdout);
    assert!(saved.starts_with("{\"version\":19,"));
    assert_eq!(
        saved
            .matches("\"evidence\":[{\"product\":\"figures\"")
            .count(),
        6
    );
    assert_eq!(
        saved
            .matches("\"evidence\":[{\"product\":\"figures\",\"entities\":{\"sub\":\"01\"}")
            .count(),
        4
    );
    assert!(saved.contains("\"name\":\"image\""));
    assert!(saved.contains("\"name\":\"metadata\""));
    for target in saved.split("\"evidence\":[").skip(1) {
        let (evidence, _) = target.split_once("],\"input\":null").unwrap();
        assert!(evidence.contains("\"product\":\"contacts\""), "{evidence}");
    }
}

#[test]
fn a_saved_dag_requires_evidence_for_each_candidate() {
    let tree = Tree::new("review-missing-evidence", &["raw/01.txt", "qc/02.txt"]);
    tree.write(
        "pipeline.spit",
        "\
source raw : T [sub]
source figures : Figure [sub]
operation fit(raw: T) -> T
image = fit(raw) @review(figures)
path raw: raw/{sub}.txt
path figures: qc/{sub}.txt
path image: image/{sub}.txt
",
    );
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\nreview image from verdicts.csv\n",
    );
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(!output.status.success());
    let diagnostic = text(&output.stderr);
    assert!(
        diagnostic
            .contains("review `image` of `image[sub=01]` has no matching evidence from `figures`"),
        "{diagnostic}"
    );
    assert!(output.stdout.is_empty());

    tree.write("qc/01.txt", "figure");
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let saved = text(&output.stdout);
    let external = saved.split_once("\"external_inputs\":").unwrap().1;
    assert!(external
        .split_once("\"targets\":")
        .unwrap()
        .0
        .contains("\"product\":\"figures\""));
}

#[test]
fn review_evidence_references_must_bind_to_a_product_and_its_entities() {
    let missing = REVIEW_EVIDENCE.replace("@review(figures, contacts)", "@review(absent)");
    assert!(errors(&missing)[0].contains("unknown evidence product `absent`"));
    let finer = REVIEW_EVIDENCE
        .replace(" @review(figures, contacts)", "")
        .replace(
            "figures = plot(image @vary(run))",
            "figures = plot(image @vary(run)) @review(image)",
        );
    let found = errors(&finer);
    assert!(
        found
            .iter()
            .any(|error| error.contains("cannot match evidence `image` by `run`")),
        "{found:?}"
    );
}

#[test]
fn evidence_in_an_operation_body_binds_to_that_call() {
    let text = "\
source raw : T [sub]
operation fit(raw: T) -> T
operation plot(image: T) -> Plot
operation prepare(raw: T) -> (image: T):
    image = fit(raw) @review(figures)
    figures = plot(image)
first = prepare(raw)
second = prepare(raw)
default path: out/{@product}/{sub}.txt
";
    assert_eq!(errors(text), Vec::<String>::new());
    let pipeline = parse_pipeline(text).unwrap();
    let evidence: Vec<_> = pipeline
        .invocations
        .iter()
        .flat_map(|invocation| &invocation.reviews)
        .flat_map(|used| used.evidence.iter().map(String::as_str))
        .collect();
    assert_eq!(evidence, ["first::figures", "second::figures"]);
}

#[test]
fn one_reviews_evidence_can_require_another_review_but_cannot_deadlock_with_it() {
    let text = "\
source raw : T [sub]
operation fit(raw: T) -> T
operation plot(a: T, b: T) -> Plot
a = fit(raw) @review(a_figures)
b = fit(raw) @review
a_figures = plot(a, b)
b_figures = plot(b, a)
default path: out/{@product}/{sub}.txt
";
    assert_eq!(errors(text), Vec::<String>::new());
    let cycle = errors(&text.replace("b = fit(raw) @review", "b = fit(raw) @review(b_figures)"));
    assert_eq!(cycle.len(), 1, "{cycle:?}");
    assert!(cycle[0].contains("review dependency cycle"));
    assert!(cycle[0].contains("review `a` of `a`"));
    assert!(cycle[0].contains("review `b` of `b`"));
}

#[test]
fn a_review_plans_every_job_whatever_its_verdicts() {
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let records =
        "sources:\n    t1w[sub=01]\n    t1w[sub=02]\n\nverdicts:\n    sums from qc/looked_at.csv\n";
    let inventory = parse_source_inventory(records).unwrap();
    let dag = resolve(&pipeline, &inventory).unwrap();
    assert_eq!(dag.jobs.len(), 3);
}

#[test]
fn the_spitout_records_only_where_each_review_file_is() {
    let records = "sources:\n    t1w[sub=01]\n\nverdicts:\n    sums from qc/looked_at.csv\n";
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let read = parse_source_inventory(records).unwrap();
    assert_eq!(read.review_files.len(), 1);
    let written = render_source_inventory(&read, &pipeline, &InputSpec::default().rules);
    assert!(
        written.ends_with("verdicts:\n    sums from qc/looked_at.csv\n"),
        "{written}"
    );
    assert_eq!(parse_source_inventory(&written).unwrap(), read);
    for bad in [
        "verdicts:\n    looked_at[sub=1]\n        verdict: pass\n",
        "verdicts:\n    looked at from qc.csv\n",
        "verdicts:\n    sums from \n",
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
    let tree = dataset("review-file", "review sums from qc/looked_at.csv\n");
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
    let tree = dataset("review-spitout", "review sums from qc/looked_at.csv\n");
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
        written.contains("verdicts:\n    sums from ../qc/looked_at.csv\n"),
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
        stderr.contains("product `nobody` has no review"),
        "{stderr}"
    );
    let tree = dataset(
        "review-twice",
        "review sums from a.csv\nreview sums from b.csv\n",
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
            stderr.contains("`exclude from` is not a recipe rule; to leave out what a person judged, add `@review` after the call that makes the product, and write `review product from file.csv` in the recipe"),
            "{command}: {stderr}"
        );
    }
}

#[test]
fn a_recipe_review_requires_a_file() {
    let tree = dataset("review-misplaced", "review sums\n");
    let (ok, _, stderr) = run(&tree, "check");
    assert!(!ok);
    assert!(
        stderr.contains("expected `review product from file.csv`"),
        "{stderr}"
    );
}
