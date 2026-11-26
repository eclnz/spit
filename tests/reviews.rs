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
fn reviews_are_declared_in_a_block_and_attached_to_products() {
    let pipeline = "\
review:
    looked_at
    second
check nonempty: test -s {@path}
source t1w : T1w [sub] @check(nonempty) @review(looked_at, second)
operation summarise(t1: T1w) -> Sum @review(looked_at, !second)
operation split(t1: T1w) -> (a: Sum @review(looked_at), b: Sum)
default review: second
";
    assert_eq!(errors(pipeline), Vec::<String>::new());
    let parsed = parse_pipeline(pipeline).unwrap();
    assert_eq!(parsed.reviews.len(), 2);
    assert_eq!(parsed.products[0].reviews, ["looked_at", "second"]);
    assert_eq!(parsed.operations[0].outputs[0].reviews, ["looked_at"]);
    assert_eq!(parsed.operations[0].outputs[0].exempt_reviews, ["second"]);
    assert_eq!(parsed.default_reviews.reviews, ["second"]);
}

#[test]
fn an_unknown_review_is_an_error_wherever_it_is_attached() {
    for (text, line) in [
        ("source t1w : T1w [sub] @review(nobody)\n", 1),
        ("operation f(x: T) -> T @review(nobody)\n", 1),
        (
            "source raw : T [sub]\noperation f(x: T) -> T\ny = f(raw) @review(nobody)\npath y: out/{sub}.txt\n",
            3,
        ),
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
fn a_review_on_either_kind_of_input_names_the_product_as_the_attachment_site() {
    for pipeline in [
        "review looked_at\noperation summarise(t1: T1w @review(looked_at)) -> Sum\n",
        "review looked_at\nsource t1w : T1w [sub]\noperation summarise(t1: T1w) -> Sum\nsums = summarise(t1w @review(looked_at))\n",
    ] {
        let found = errors(pipeline);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("attach it to the source or operation output"));
    }
}

#[test]
fn a_review_suffix_on_a_multi_output_call_is_ambiguous() {
    let found = errors("review looked_at\nsource raw : T [sub]\noperation f(x: T) -> (a: T, b: T)\ny, z = f(raw) @review(looked_at)\n");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("select the reviewed product with"));
}

const REVIEW_EVIDENCE: &str = "\
review looked_at
source raw : T [sub, run]
operation fit(raw: T) -> (image: T, metadata: T)
operation plot(images: many T) -> Plot
image, metadata = fit(raw) @review(looked_at, product: image, evidence: figures)
figures = plot(image @vary(run))
default path: out/{@product}/{sub}[/{run}].txt
";

#[test]
fn a_review_names_forward_evidence_and_selects_the_reviewed_output() {
    assert_eq!(errors(REVIEW_EVIDENCE), Vec::<String>::new());
    let pipeline = parse_pipeline(REVIEW_EVIDENCE).unwrap();
    let used = &pipeline.invocations[0].reviews[0];
    assert_eq!(used.name, "looked_at");
    assert_eq!(used.port, spit::Port::Output(0));
    assert_eq!(used.evidence.as_deref(), Some("figures"));
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
        "pipeline pipeline.spit\nroot .\nreview looked_at from qc.csv\n",
    );
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let saved = text(&output.stdout);
    assert!(saved.starts_with("{\"version\":19,"));
    assert_eq!(
        saved
            .matches("\"evidence\":{\"product\":\"figures\"")
            .count(),
        3
    );
    assert_eq!(
        saved
            .matches("\"evidence\":{\"product\":\"figures\",\"entities\":{\"sub\":\"01\"}")
            .count(),
        2
    );

    tree.write(
        "pipeline.spit",
        &format!(
            "{}path raw: in/{{sub}}/{{run}}.txt\n",
            REVIEW_EVIDENCE.replace("product: image, evidence: figures", "product: metadata")
        ),
    );
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains(
        "\"dimensions\":[\"sub\",\"run\"],\"targets\":[{\"artifact\":{\"product\":\"metadata\""
    ));
}

#[test]
fn a_saved_dag_requires_evidence_for_each_candidate() {
    let tree = Tree::new("review-missing-evidence", &["raw/01.txt", "qc/02.txt"]);
    tree.write(
        "pipeline.spit",
        "\
review looked_at
source raw : T [sub]
source figures : Figure [sub]
operation fit(raw: T) -> T
image = fit(raw) @review(looked_at, evidence: figures)
path raw: raw/{sub}.txt
path figures: qc/{sub}.txt
path image: image/{sub}.txt
",
    );
    let recipe = tree.write(
        "data.spitin",
        "pipeline pipeline.spit\nroot .\nreview looked_at from verdicts.csv\n",
    );
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(!output.status.success());
    let diagnostic = text(&output.stderr);
    assert!(
        diagnostic.contains(
            "review `looked_at` of `image[sub=01]` has no matching evidence from `figures`"
        ),
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
    let missing = REVIEW_EVIDENCE.replace("evidence: figures", "evidence: absent");
    assert!(errors(&missing)[0].contains("unknown evidence product `absent`"));
    let finer = REVIEW_EVIDENCE
        .replace("product: image", "product: figures")
        .replace(
            "figures = plot(image @vary(run))",
            "figures = plot(image @vary(run)) @review(looked_at, evidence: image)",
        )
        .replace(
            " @review(looked_at, product: figures, evidence: figures)",
            "",
        );
    let found = errors(&finer);
    assert!(
        found
            .iter()
            .any(|error| error.contains("cannot match evidence `image` by `run`")),
        "{found:?}"
    );
    let wrong_output = REVIEW_EVIDENCE.replace("product: image", "product: raw");
    assert!(errors(&wrong_output)[0].contains("this call does not produce `raw`"));
    let repeated = REVIEW_EVIDENCE.replace(
        "evidence: figures)",
        "evidence: figures) @review(looked_at, product: image, evidence: figures)",
    );
    assert!(errors(&repeated)[0].contains("declares evidence more than once"));
}

#[test]
fn evidence_in_an_operation_body_binds_to_that_call() {
    let text = "\
review looked_at
source raw : T [sub]
operation fit(raw: T) -> T
operation plot(image: T) -> Plot
operation prepare(raw: T) -> (image: T):
    image = fit(raw) @review(looked_at, evidence: figures)
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
        .filter_map(|used| used.evidence.as_deref())
        .collect();
    assert_eq!(evidence, ["first::figures", "second::figures"]);
}

#[test]
fn one_reviews_evidence_can_require_another_review_but_cannot_deadlock_with_it() {
    let text = "\
review first
review second
source raw : T [sub]
operation fit(raw: T) -> T
operation plot(a: T, b: T) -> Plot
a = fit(raw) @review(first, evidence: a_figures)
b = fit(raw) @review(second)
a_figures = plot(a, b)
b_figures = plot(b, a)
default path: out/{@product}/{sub}.txt
";
    assert_eq!(errors(text), Vec::<String>::new());
    let cycle = errors(&text.replace("@review(second)", "@review(second, evidence: b_figures)"));
    assert_eq!(cycle.len(), 1, "{cycle:?}");
    assert!(cycle[0].contains("review dependency cycle"));
    assert!(cycle[0].contains("review `first` of `a`"));
    assert!(cycle[0].contains("review `second` of `b`"));
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
        "review looked_at\nsource ref : T [sub] @review(looked_at)\noperation f(x: T) -> T @review(looked_at)\n",
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
    assert_eq!(
        pipeline.operations[0].outputs[0].reviews,
        ["lib::looked_at"]
    );
    assert_eq!(pipeline.products[0].reviews, ["lib::looked_at"]);
}
