//! An error in an imported file's own text is reported in that file, at its
//! own line and columns, by its path from the pipeline's folder, with the
//! `use` line that reads it as a related place.

mod support;

use support::Tree;

const OPERATIONS: &str = "operation cp(a: Lines) -> Lines\ncommand cp: cp {a} {@output}\n";

/// A pipeline that imports `wrap` from `libs/lib.spit`, with a bad line of
/// its own after the import.
fn tree(library: &str) -> (Tree, String) {
    let tree = Tree::new("imported-errors", &[]);
    tree.write("libs/lib.spit", library);
    let main = tree.write(
        "pipeline.spit",
        "use wrap from libs/lib.spit\nsource raw : Lines [id]\nout = wrap(raw)\nfoo bar baz\n",
    );
    let main = main.to_str().unwrap().to_owned();
    (tree, main)
}

fn check(main: &str, json: bool) -> String {
    let args: &[&str] = if json {
        &["check", main, "--json"]
    } else {
        &["check", main]
    };
    let output = support::spit(args);
    support::text(if json { &output.stdout } else { &output.stderr })
}

/// What the library's error is reported as, and that the pipeline's own bad
/// line is still reported, once, without a repeat of the missing import.
fn assert_reported(library: &str, text: &str, json: &[&str]) {
    let (tree, main) = tree(library);
    let shown = check(&main, false);
    assert!(shown.contains(text), "{shown}");
    assert!(
        shown.contains("error: line 4, column 1: `foo` does not start a statement"),
        "{shown}"
    );
    assert_eq!(shown.matches("error: ").count(), 2, "{shown}");
    assert!(!shown.contains(tree.path().to_str().unwrap()), "{shown}");
    let shown = check(&main, true);
    for part in json {
        assert!(shown.contains(part), "{shown}");
    }
    assert!(shown.contains("\"line\":4,\"column\":1"), "{shown}");
}

#[test]
fn a_body_that_reads_a_missing_product_is_reported_in_the_library() {
    let library = format!(
        "{OPERATIONS}operation wrap(x: Lines) -> (out: Lines):\n    out = cp(x)\n    z = cp(nope)\n"
    );
    assert_reported(
        &library,
        "error: libs/lib.spit: line 5, column 9: the body of `wrap` reads `nope`, which is neither one of its inputs nor made by an earlier step of it\n  \
         --> pipeline.spit: line 1, column 1: imported here\n",
        &[
            "\"line\":5,\"column\":9,\"end_column\":17,\"message\":\"the body of `wrap` reads `nope`, which is neither one of its inputs nor made by an earlier step of it\",\
\"file\":\"libs/lib.spit\",\"related\":[{\"file\":\"pipeline.spit\",\"line\":1,\"column\":1,\"end_column\":28,\"message\":\"imported here\"}]",
        ],
    );
}

#[test]
fn an_undeclared_operation_is_reported_in_the_library() {
    let library =
        format!("{OPERATIONS}operation wrap(x: Lines) -> (out: Lines):\n    out = missing(x)\n");
    assert_reported(
        &library,
        "error: libs/lib.spit: line 4, column 11: operation `missing` must be declared before `wrap`, whose body calls it\n  \
         --> pipeline.spit: line 1, column 1: imported here\n",
        &["\"file\":\"libs/lib.spit\""],
    );
}

#[test]
fn a_bad_placeholder_is_reported_in_the_library() {
    let library = "operation cp(a: Lines) -> Lines\ncommand cp: cp {a} {@output} {oops\n\
                   operation wrap(x: Lines) -> Lines\n";
    assert_reported(
        library,
        "error: libs/lib.spit: line 2, column 13: command `cp`: unclosed `{` in `{oops`\n  \
         --> pipeline.spit: line 1, column 1: imported here\n",
        &[
            "\"line\":2,\"column\":13,\"end_column\":35,\"message\":\"command `cp`: unclosed `{` in `{oops`\",\"file\":\"libs/lib.spit\"",
        ],
    );
}

#[test]
fn a_library_that_imports_a_broken_library_names_each_use_line() {
    let tree = Tree::new("imported-errors-nested", &[]);
    tree.write(
        "a/b/lib.spit",
        &format!("{OPERATIONS}operation wrap(x: Lines) -> (out: Lines):\n    out = cp(nope)\n"),
    );
    tree.write("a/mid.spit", "# the middle\nuse wrap from b/lib.spit\n");
    let main = tree.write("p.spit", "use wrap from a/mid.spit\n");
    let main = main.to_str().unwrap();
    let shown = check(main, false);
    assert_eq!(
        shown,
        "error: a/b/lib.spit: line 4, column 11: the body of `wrap` reads `nope`, which is neither one of its inputs nor made by an earlier step of it\n  \
         --> a/mid.spit: line 2, column 1: imported here\n  \
         --> p.spit: line 1, column 1: imported here\n"
    );
    let shown = check(main, true);
    assert!(
        shown.contains(
            "\"file\":\"a/b/lib.spit\",\"related\":[\
{\"file\":\"a/mid.spit\",\"line\":2,\"column\":1,\"end_column\":25,\"message\":\"imported here\"},\
{\"file\":\"p.spit\",\"line\":1,\"column\":1,\"end_column\":25,\"message\":\"imported here\"}]"
        ),
        "{shown}"
    );
}

#[test]
fn a_library_error_does_not_hide_an_error_on_the_same_line_of_the_pipeline() {
    let (tree, main) = tree(&format!(
        "{OPERATIONS}operation wrap(x: Lines) -> (out: Lines):\n    out = cp(nope)\n"
    ));
    // The pipeline's line 4 and the library's line 4 are different places.
    let shown = check(&main, false);
    assert!(shown.contains("libs/lib.spit: line 4"), "{shown}");
    assert!(shown.contains("error: line 4, column 1"), "{shown}");
    drop(tree);
}

#[test]
fn a_recipe_names_its_pipelines_library_relative_to_the_recipe() {
    let tree = Tree::new("imported-errors-recipe", &[]);
    tree.write(
        "pipelines/libs/lib.spit",
        &format!("{OPERATIONS}operation wrap(x: Lines) -> (out: Lines):\n    out = cp(nope)\n"),
    );
    tree.write("pipelines/pipeline.spit", "use wrap from libs/lib.spit\n");
    let recipe = tree.write(
        "recipes/check.spitin",
        "pipeline ../pipelines/pipeline.spit\nroot .\n",
    );
    let recipe = recipe.to_str().unwrap();
    let text = check(recipe, false);
    assert!(
        text.contains("error: ../pipelines/libs/lib.spit: line 4, column 11:"),
        "{text}"
    );
    assert!(
        text.contains("--> ../pipelines/pipeline.spit: line 1, column 1: imported here"),
        "{text}"
    );
    assert!(!text.contains(tree.path().to_str().unwrap()), "{text}");
    let json = check(recipe, true);
    assert!(
        json.contains("\"file\":\"../pipelines/libs/lib.spit\""),
        "{json}"
    );
    assert!(
        json.contains("\"related\":[{\"file\":\"../pipelines/pipeline.spit\""),
        "{json}"
    );
    assert!(!json.contains(tree.path().to_str().unwrap()), "{json}");
}
