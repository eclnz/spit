//! `@join` and `@list`: a `many` input's paths as one argument, in natural
//! order, as the `.spitdag` and `dag --commands` give them.

mod support;

use support::{spit, text, Tree};

/// A pipeline gathering `in/10.txt` and `in/2.txt` with `command`, and its
/// path.
fn pipeline(command: &str) -> (Tree, String) {
    let tree = Tree::new("collection-arguments", &["data/in/10.txt", "data/in/2.txt"]);
    let file = tree.write(
        "main.spit",
        &format!("root data\nsource raw: Text [n]\npath raw: in/{{n}}.txt\noperation gather(xs: many Text, one: Text) -> Text .txt\ncommand gather: {command}\nresult = gather(raw @vary(n), raw @where(n=2))\npath result: result.txt\n"),
    );
    (tree, file.to_str().unwrap().to_owned())
}

#[test]
fn a_join_and_a_list_keep_their_artifacts_in_natural_order() {
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
    let commands = text(&spit(&["dag", &file, "--commands"]).stdout);
    assert!(
        commands.contains("--joined=in/2.txt,in/10.txt '2; 10' '@list(in/2.txt, in/10.txt)'"),
        "{commands}"
    );
}

#[test]
fn a_separator_may_hold_quotes_parentheses_and_spaces() {
    for (written, separator) in [
        ("", ""),
        (" ", " "),
        (")", ")"),
        ("\\\"", "\""),
        ("\\\\", "\\\\"),
        ("a,b", "a,b"),
        ("\\t", "\\t"),
    ] {
        let (_tree, file) = pipeline(&format!("tool @join(xs, \"{written}\") {{@output}}"));
        let output = spit(&["dag", &file, "--json"]);
        let json = text(&output.stdout);
        assert!(
            output.status.success(),
            "{written}: {}",
            text(&output.stderr)
        );
        assert!(
            json.contains(&format!(
                "\"separator\":\"{}\"",
                separator.replace('"', "\\\"")
            )),
            "{written}: {json}"
        );
    }
}

#[test]
fn each_mistake_says_what_is_wrong() {
    for (command, message) in [
        (
            "tool @join(xs, x) {@output}",
            "write the separator in double quotes",
        ),
        ("tool @list(xs.n) {@output}", "as in `@list(images)`"),
        (
            "tool @list(xs, \",\") {@output}",
            "`@list` takes only a `many` input port",
        ),
        (
            "tool @join(unknown) {@output}",
            "has no `many` input port `unknown`",
        ),
        ("tool @join(one) {@output}", "`one` is a `one` input"),
        (
            "tool @join(xs.dir) {@output}",
            "not their `.dir` or `.stem`",
        ),
        ("tool @join(xs.site) {@output}", "site"),
        (
            "@join(xs) {@output}",
            "a command starts with the program it runs",
        ),
        ("tool @join(xs, \",\" {@output}", "is never closed"),
        (
            "tool @join(xs.n, \"2\") {@output}",
            "the value of `raw[n=2]`, `2`, holds the separator `2`",
        ),
        (
            "tool @join(xs, \"/\") {@output}",
            "the path of `raw[n=2]`, `in/2.txt`, holds the separator `/`",
        ),
    ] {
        let (_tree, file) = pipeline(command);
        let output = spit(&["dag", &file, "--json"]);
        let stderr = text(&output.stderr);
        assert!(
            !output.status.success(),
            "{command}: {}",
            text(&output.stdout)
        );
        assert!(stderr.contains(message), "{command}: {stderr}");
    }
    let (tree, file) = pipeline("tool {xs} {@output}");
    let checked = std::fs::read_to_string(&file).unwrap()
        + "check listed: tool @list(xs) {@path}\nsource extra: Text @check(listed)\npath extra: in/2.txt\n";
    tree.write("main.spit", &checked);
    let stderr = text(&spit(&["check", &file]).stderr);
    assert!(
        stderr.contains("check `listed` cannot use `@list(xs)`"),
        "{stderr}"
    );
}

#[test]
fn quoted_it_is_text() {
    let (_tree, file) = pipeline("tool '@join(xs)' \"@list(xs)\" {@output}");
    let json = text(&spit(&["dag", &file, "--json"]).stdout);
    assert!(json.contains("[\"@join(xs)\"],[\"@list(xs)\"]"), "{json}");
}
