//! Collection argument structure, validation and deterministic ordering.

mod support;
use support::{spit, text, Tree};

fn pipeline(command: &str) -> (Tree, String) {
    let tree = Tree::new("collection-arguments", &["data/in/10.txt", "data/in/2.txt"]);
    let file = tree.write("main.spit", &format!("root data\nsource raw: Text [n]\npath raw: in/{{n}}.txt\noperation gather(xs: many Text) -> Text .txt\ncommand gather: {command}\nresult = gather(raw @vary(n))\npath result: result.txt\n"));
    (tree, file.to_str().unwrap().to_owned())
}

#[test]
fn joining_and_lists_keep_artifact_references_and_order() {
    let (_tree, file) =
        pipeline("tool {xs} --joined=@join(xs) @join(xs.n, \"; \" ) @list(xs) {@output}");
    let output = spit(&["dag", &file, "--json"]);
    let json = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(
        json.contains(
            "[\"--joined=\",{\"join\":[\"in/2.txt\",\"in/10.txt\"],\"separator\":\",\"}]"
        ),
        "{json}"
    );
    assert!(json.contains("[\"2; 10\"]"), "{json}");
    assert!(
        json.contains("{\"list\":[\"in/2.txt\",\"in/10.txt\"]}"),
        "{json}"
    );
    let output = spit(&["dag", &file, "--commands"]);
    let commands = text(&output.stdout);
    assert!(
        commands.contains("--joined=in/2.txt,in/10.txt"),
        "{commands}"
    );
}

#[test]
fn separators_preserve_quotes_parentheses_and_whitespace() {
    for separator in ["", " ", ")", "\\\"", "\\\\", "a,b"] {
        let (_tree, file) = pipeline(&format!("tool @join(xs, \"{separator}\") {{@output}}"));
        let output = spit(&["check", &file]);
        assert!(output.status.success(), "{}", text(&output.stderr));
    }
}

#[test]
fn malformed_collection_functions_and_colliding_values_fail() {
    for command in [
        "tool @join(xs, x) {@output}",
        "tool @list(xs.n) {@output}",
        "tool @list(xs, \",\") {@output}",
        "tool @join(unknown) {@output}",
        "@join(xs) {@output}",
        "tool @join(xs, \",\" {@output}",
        "tool @join(xs.n, \"2\") {@output}",
        "tool @join(xs, \"/\") {@output}",
    ] {
        let (_tree, file) = pipeline(command);
        let output = spit(&["dag", &file, "--json"]);
        assert!(
            !output.status.success(),
            "{command}: {}",
            text(&output.stdout)
        );
    }
}

#[test]
fn anchored_mark_reaches_the_dag_and_hover() {
    let (tree, file) = pipeline("tool {xs} {@output}");
    let source = std::fs::read_to_string(&file).unwrap() + "anchored gather\n";
    tree.write("main.spit", &source);
    let output = spit(&["dag", &file, "--json"]);
    let json = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(json.contains("\"anchored\":true"), "{json}");
    let output = spit(&["check", &file, "--json", "--hovers"]);
    assert!(text(&output.stdout).contains("Anchored:"));
}
