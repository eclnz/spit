//! The note that lists values differing only in ASCII letter case, which a
//! recipe's settled inputs give, and what it leaves out.

mod support;

use std::process::Output;

use support::{spit, Tree};

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn inputs_lists_values_that_differ_only_in_case_together() {
    let recipe = format!(
        "{}/tests/fixtures/weekly_stores/weekly.spitin",
        env!("CARGO_MANIFEST_DIR")
    );
    let note = "note: `store` has values that differ only in ASCII letter case, which are different values to SPIT: `S07` in pricing, `s07` in sales\n";
    for command in ["inputs", "dag", "artifacts"] {
        let output = spit(&[command, &recipe]);
        assert_eq!(stderr(&output).matches(note).count(), 1, "{command}");
    }
}

#[test]
fn a_group_exclusion_still_shows_the_spelling_it_missed() {
    let root = format!(
        "{}/tests/fixtures/weekly_stores",
        env!("CARGO_MANIFEST_DIR")
    );
    let tree = support::Tree::new("case-variants", &[]);
    let recipe = tree.write(
        "dataset.spitin",
        &format!("pipeline {root}/pipeline.spit\nroot {root}\nexclude [store=s07]\n"),
    );
    let output = spit(&["inputs", recipe.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("`S07` in pricing, `s07` (excluded)\n"));

    let both = tree.write(
        "both.spitin",
        &format!(
            "pipeline {root}/pipeline.spit\nroot {root}\nexclude [store=s07]\nexclude pricing[store=S07]\n"
        ),
    );
    let output = spit(&["inputs", both.to_str().unwrap()]);
    assert!(
        !stderr(&output).contains("letter case"),
        "{}",
        stderr(&output)
    );
}

/// A sales pipeline with one source, and a recipe over `files` with `rules`.
fn sales_recipe(name: &str, files: &[&str], rules: &str) -> (Tree, String) {
    let tree = Tree::new(name, files);
    // Keep case variants in separate week folders so both files exist on
    // filesystems that ignore case, while the scanner retains each spelling.
    tree.write(
        "pipeline.spit",
        "source sales : Sales [store, week]\npath sales: sales/{week}/{store}.csv\noperation clean(sales: Sales) -> Sales\ncommand clean: clean_sales {sales} {@output}\npath cleaned: out/{store}/{week}.csv\ncleaned = clean(sales)\n",
    );
    let recipe = tree.write(
        "dataset.spitin",
        &format!("pipeline pipeline.spit\nroot .\n{rules}"),
    );
    let recipe = recipe.to_str().unwrap().to_owned();
    (tree, recipe)
}

#[test]
fn a_partial_exclusion_lists_the_spelling_it_left_with_its_remaining_sources() {
    let (_tree, recipe) = sales_recipe(
        "case-partial",
        &["sales/w1/s01.csv", "sales/w2/s01.csv", "sales/w3/S01.csv"],
        "exclude [store=s01,week=w1]\n",
    );
    let output = spit(&["inputs", &recipe]);
    assert!(output.status.success(), "{}", stderr(&output));
    // `s01` keeps week 2, so it is listed with its source and is not marked
    // `(excluded)`, which is for a spelling with nothing left.
    assert!(
        stderr(&output)
            .contains("which are different values to SPIT: `S01` in sales, `s01` in sales\n"),
        "{}",
        stderr(&output)
    );
    assert!(!stderr(&output).contains("(excluded)"));
    assert!(stdout(&output).contains("sales[store=S01,week=w3]"));
    assert!(stdout(&output).contains("sales[store=s01,week=w2]"));
    assert!(!stdout(&output).contains("sales[store=s01,week=w1]"));
}

#[test]
fn values_that_differ_only_outside_ascii_get_no_case_note() {
    let (_tree, recipe) = sales_recipe("case-unicode", &["sales/w1/é1.csv", "sales/w2/É1.csv"], "");
    let output = spit(&["inputs", &recipe]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        !stderr(&output).contains("letter case"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_spitout_given_directly_gets_no_case_note() {
    let (tree, recipe) = sales_recipe(
        "case-spitout",
        &["sales/w1/s01.csv", "sales/w2/S01.csv"],
        "",
    );
    let inputs = spit(&["inputs", &recipe]);
    assert!(stderr(&inputs).contains("differ only in ASCII letter case"));
    let spitout = tree.write("dataset.spitout", &stdout(&inputs));
    let pipeline = tree.path().join("pipeline.spit");
    for command in ["dag", "artifacts"] {
        let output = spit(&[
            command,
            pipeline.to_str().unwrap(),
            spitout.to_str().unwrap(),
        ]);
        assert!(
            !stderr(&output).contains("ASCII letter case"),
            "{command}: {}",
            stderr(&output)
        );
    }
}
