mod support;

use support::{spit, text, Tree};

const PIPELINE: &str = "\
source t1w : T1w [sub]
operation mask(t1: T1w) -> Mask
operation fit(mask: Mask) -> Fit
operation report(fit: Fit) -> Report
operation table(fits: many Fit) -> Table
path t1w: sub-{sub}/t1w.txt
path masks: out/mask-{sub}.txt
path fits: out/fit-{sub}.txt
path reports: out/report-{sub}.txt
path total: out/table.txt
masks = mask(t1w) @review
fits = fit(masks)
reports = report(fits)
total = table(fits @vary(sub))
";

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

fn dag(tree: &Tree) -> (String, String) {
    let recipe = tree.path().join("data.spitin");
    let output = spit(&["dag", recipe.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    (text(&output.stdout), text(&output.stderr))
}

fn reviews(json: &str) -> &str {
    let start = json.find(",\"reviews\":").expect("a reviews field") + 1;
    let end = json[start..]
        .find(",\"pipeline_folder\":")
        .expect("pipeline_folder after reviews");
    &json[start..start + end]
}

fn file(tree: &Tree, name: &str) -> String {
    let folder = std::fs::canonicalize(tree.path()).unwrap();
    format!("\"{}/{name}\"", folder.display())
}

const RECIPE: &str = "review masks from qc/looked_at.csv\n";

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
fn a_review_can_use_a_source_as_evidence() {
    let source = PIPELINE.replace(
        "masks = mask(t1w) @review",
        "masks = mask(t1w) @review(t1w)",
    );
    let tree = dataset("snapshot-source-evidence", &source, RECIPE);
    let (json, _) = dag(&tree);
    assert!(
        reviews(&json)
            .contains(r#""path":"sub-03/t1w.txt","under":"root","kind":"file"}],"input":null"#),
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
    assert!(
        written.contains("verdicts:\n    masks from ../qc/looked_at.csv\n"),
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
