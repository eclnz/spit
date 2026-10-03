//! What the pipeline and the recipe each hold: a declaration written in the
//! wrong file says which file owns it, and a source's path rule comes from
//! whichever file gives it, by a fixed precedence.

mod support;

use support::{spit, text, Tree};

use spit::{parse_input_spec, parse_pipeline, InputSource};

const PIPELINE: &str = "\
path: def/{@product}/{@entities}.txt
source a [sub]
path a: own/a-{sub}.txt
source b [sub]
source c [sub]
operation f(a, b, c) -> Out
out = f(a, b, c)
";

/// The path of each source record a scan of `files` finds with `recipe`.
fn found(recipe: &str, files: &[&str]) -> Vec<String> {
    let tree = Tree::new("two-files-precedence", files);
    let pipeline = parse_pipeline(PIPELINE).unwrap();
    let resolved = parse_input_spec(recipe)
        .unwrap()
        .resolve(&pipeline, InputSource::Discover(tree.path()))
        .unwrap();
    resolved
        .inventory
        .artifacts
        .iter()
        .map(|record| record.path.clone().unwrap())
        .collect()
}

#[test]
fn a_source_takes_its_own_rule_then_the_recipes_default_then_the_pipelines() {
    // `a` has its own rule in the pipeline and `b` in the recipe; `c` has
    // none, so the recipe's default covers it, though the pipeline has one.
    let files = [
        "own/a-1.txt",
        "rec/b-1.txt",
        "rdef/c-1.txt",
        "def/c/sub=1.txt",
    ];
    assert_eq!(
        found(
            "path b: rec/b-{sub}.txt\npath: rdef/{@product}-{sub}.txt\n",
            &files
        ),
        ["own/a-1.txt", "rec/b-1.txt", "rdef/c-1.txt"]
    );
    // Without a recipe default, the pipeline's covers `c`.
    assert_eq!(
        found("path b: rec/b-{sub}.txt\n", &files),
        ["own/a-1.txt", "rec/b-1.txt", "def/c/sub=1.txt"]
    );
}

/// The errors `spit check` gives for a recipe beside `PIPELINE`.
fn recipe_errors(recipe: &str) -> String {
    let tree = Tree::new("two-files-recipe", &[]);
    tree.write("pipeline.spit", PIPELINE);
    let recipe = tree.write(
        "dataset.spitin",
        &format!("pipeline pipeline.spit\nroot .\n{recipe}"),
    );
    let output = spit(&["check", recipe.to_str().unwrap()]);
    assert!(!output.status.success());
    text(&output.stderr)
}

#[test]
fn a_recipe_line_that_belongs_in_the_pipeline_says_so_at_its_line() {
    for (line, message) in [
        (
            "source d [sub]",
            "error: line 3, column 1: `source` belongs in the .spit pipeline, which every dataset shares",
        ),
        (
            "command f: run {a}",
            "error: line 3, column 1: `command` belongs in the .spit pipeline",
        ),
        (
            "more = f(a, b, c)",
            "error: line 3, column 1: a step belongs in the .spit pipeline",
        ),
        (
            "(x, y) = f(a, b, c)",
            "error: line 3, column 1: a step belongs in the .spit pipeline",
        ),
        (
            "path out: x/{sub}.txt",
            "error: line 3, column 11: `out` is made by a step, so its path belongs in the .spit pipeline",
        ),
        (
            "path a: x/{sub}.txt",
            "error: line 3, column 9: `a` has path rules in both .spit and .spitin; keep the pipeline's if every dataset has this layout, or the recipe's if only this one does",
        ),
    ] {
        let errors = recipe_errors(&format!("{line}\n"));
        assert!(errors.contains(message), "{line}: {errors}");
    }
}

#[test]
fn a_recipe_line_in_a_pipeline_says_where_it_belongs() {
    let tree = Tree::new("two-files-pipeline", &[]);
    for (line, word) in [("root data", "root"), ("pipeline other.spit", "pipeline")] {
        let pipeline = tree.write("pipeline.spit", &format!("{line}\n{PIPELINE}"));
        let output = spit(&["check", pipeline.to_str().unwrap()]);
        assert!(!output.status.success());
        let errors = text(&output.stderr);
        assert!(
            errors.contains(&format!(
                "error: line 1, column 1: `{word}` belongs in a .spitin recipe, which names its pipeline and the dataset folder it is bound to; a pipeline given alone takes its folder from `--root`"
            )),
            "{errors}"
        );
    }
}
