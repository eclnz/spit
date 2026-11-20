//! A saved DAG identifies potential review targets and their scope.

mod support;

use support::{spit, text, Tree};

/// Masks a person reviews, then a fit and a report per subject from each
/// mask, and a table of every fit.
const PIPELINE: &str = "\
review looked_at
source t1w : T1w [sub]
operation mask(t1: T1w) -> Mask @review(looked_at)
operation fit(mask: Mask) -> Fit
operation report(fit: Fit) -> Report
operation table(fits: many Fit) -> Table
path t1w: sub-{sub}/t1w.txt
path masks: out/mask-{sub}.txt
path fits: out/fit-{sub}.txt
path reports: out/report-{sub}.txt
path total: out/table.txt
masks = mask(t1w)
fits = fit(masks)
reports = report(fits)
total = table(fits @vary(sub))
";

/// Subjects 01 to 03 under the pipeline `pipeline`, with the recipe lines
/// `recipe`.
fn dataset(name: &str, pipeline: &str, recipe: &str) -> Tree {
    let tree = Tree::new(
        name,
        &["sub-01/t1w.txt", "sub-02/t1w.txt", "sub-03/t1w.txt"],
    );
    tree.write("pipeline.spit", pipeline);
    tree.write(
        "data.spitin",
        &format!("pipeline pipeline.spit\nroot .\n{recipe}"),
    );
    tree
}

/// The `.spitdag` of `tree`'s recipe, and what `spit dag` noted.
fn dag(tree: &Tree) -> (String, String) {
    let recipe = tree.path().join("data.spitin");
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    (text(&output.stdout), text(&output.stderr))
}

/// The `reviews` field of a `.spitdag`.
fn reviews(json: &str) -> &str {
    let start = json.find(",\"reviews\":").expect("a reviews field") + 1;
    let end = json[start..]
        .find(",\"pipeline_folder\":")
        .expect("pipeline_folder after reviews");
    &json[start..start + end]
}

/// The file `name` under `tree`, as a `.spitdag` writes it.
fn file(tree: &Tree, name: &str) -> String {
    let folder = std::fs::canonicalize(tree.path()).unwrap();
    format!("\"{}/{name}\"", folder.display())
}

const RECIPE: &str = "review looked_at from qc/looked_at.csv\n";

#[test]
fn each_potential_review_target_is_present_regardless_of_its_verdict() {
    let tree = dataset("snapshot-mixed", PIPELINE, RECIPE);
    let (before, _) = dag(&tree);
    tree.write(
        "qc/looked_at.csv",
        "sub,verdict,reason\n01,pass,\n02,fail,ghosting\n",
    );
    let (json, stderr) = dag(&tree);
    let reviewed = reviews(&json);
    assert_eq!(reviews(&before), reviewed);
    assert!(reviewed.contains(&format!(
        r#""file":{},"dimensions":["sub"]"#,
        file(&tree, "qc/looked_at.csv")
    )));
    assert_eq!(reviewed.matches(r#""input":null"#).count(), 3);
    // The review file sits in the dataset, and is no source to report.
    assert!(!stderr.contains("matches no source rule"), "{stderr}");
}

#[test]
fn a_named_missing_file_is_distinct_from_a_review_with_no_file() {
    let tree = dataset("snapshot-missing", PIPELINE, RECIPE);
    let (json, _) = dag(&tree);
    assert!(reviews(&json).contains(&file(&tree, "qc/looked_at.csv")));
    let tree = dataset("snapshot-unnamed", PIPELINE, "");
    let (json, _) = dag(&tree);
    assert!(reviews(&json).contains(r#""file":null"#));
}

#[test]
fn a_review_on_an_input_port_names_it_and_a_source_gives_its_path() {
    let port = PIPELINE
        .replace(" -> Mask @review(looked_at)", " -> Mask")
        .replace("fit(mask: Mask)", "fit(mask: Mask @review(looked_at))");
    let tree = dataset("snapshot-port", &port, RECIPE);
    tree.write("qc/looked_at.csv", "sub,verdict\n01,pass\n02,pass\n");
    let (json, _) = dag(&tree);
    assert!(
        reviews(&json).contains(r#""path":"out/mask-03.txt","under":"output_root","kind":"file"},"input":{"job":6,"port":0}"#),
        "{json}"
    );

    let source = PIPELINE
        .replace(" -> Mask @review(looked_at)", " -> Mask")
        .replace(
            "source t1w : T1w [sub]",
            "source t1w : T1w [sub] @review(looked_at)",
        );
    let tree = dataset("snapshot-source", &source, RECIPE);
    tree.write("qc/looked_at.csv", "sub,verdict\n01,pass\n02,pass\n");
    let (json, _) = dag(&tree);
    assert!(
        reviews(&json)
            .contains(r#""path":"sub-03/t1w.txt","under":"root","kind":"file"},"input":null"#),
        "{json}"
    );
}

#[test]
fn a_dag_made_from_a_spitout_records_the_same_reviews() {
    let tree = dataset("snapshot-spitout", PIPELINE, RECIPE);
    tree.write("qc/looked_at.csv", "sub,verdict\n01,pass\n02,fail\n");
    let (from_recipe, _) = dag(&tree);
    let recipe = tree.path().join("data.spitin");
    std::fs::create_dir_all(tree.path().join("saved")).unwrap();
    let spitout = tree.path().join("saved/data.spitout");
    let output = spit(&[
        "inputs",
        recipe.to_str().unwrap(),
        "-o",
        spitout.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let written = std::fs::read_to_string(&spitout).unwrap();
    // Relative to the .spitout, as its root is.
    assert!(
        written.contains("verdicts:\n    looked_at from ../qc/looked_at.csv\n"),
        "{written}"
    );
    let pipeline = tree.path().join("pipeline.spit");
    let output = spit(&[
        "dag",
        pipeline.to_str().unwrap(),
        spitout.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(reviews(&text(&output.stdout)), reviews(&from_recipe));

    std::fs::remove_file(tree.path().join("qc/looked_at.csv")).unwrap();
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(
        text(&output.stdout).contains("qc/looked_at.csv\n"),
        "{}",
        text(&output.stdout)
    );
}
