//! `here` sources: files kept beside the pipeline, found from the folder of
//! the file that declares them and read as inputs from `pipeline_folder`.

mod support;

use support::{spit, text, Tree};

const LIBRARY: &str = "source shouter : Script .sh here
path shouter: tools/shout.sh
operation shout(t: Lines, s: Script) -> Lines
command shout: sh {s} {t} > {@output}
";

const MAIN: &str = "use libs/lib.spit
root data
source s : Lines [id]
source lower : Script .sh here
path s: in/{id}.txt
path lower: scripts/low.sh
operation low(t: Lines, script: Script) -> Lines
command low: sh {script} {t} > {@output}
default entities: {@key}={@value} separated \"_\"
default path: out/{@product}/{@entities}.txt
a = low(s, lower)
b = shout(a, shouter)
";

/// A pipeline, a library beside it, their scripts and a dataset, with
/// `main.spit` holding `main`.
fn pipeline(name: &str, main: &str) -> (Tree, String) {
    let tree = Tree::new(
        name,
        &["data/in/x.txt", "scripts/low.sh", "libs/tools/shout.sh"],
    );
    tree.write("libs/lib.spit", LIBRARY);
    let main = tree.write("main.spit", main);
    (tree, main.to_str().unwrap().to_owned())
}

#[test]
fn a_here_source_is_an_input_under_the_pipeline_folder() {
    let (tree, main) = pipeline("here-sources", MAIN);
    let output = spit(&["dag", &main, "--json"]);
    let json = text(&output.stdout);
    assert!(output.status.success(), "{}", text(&output.stderr));
    for (product, path) in [
        ("lower", "scripts/low.sh"),
        ("shouter", "libs/tools/shout.sh"),
    ] {
        assert!(
            json.contains(&format!("\"product\":\"{product}\",\"entities\":{{}},\"type\":{{\"name\":\"Script\",\"args\":[]}},\"path\":\"{path}\",\"under\":\"pipeline_folder\"")),
            "{json}"
        );
    }
    let folder = std::fs::canonicalize(tree.path()).unwrap();
    let commands = text(&spit(&["dag", &main, "--commands"]).stdout);
    assert!(
        commands.contains(&format!("sh {}/scripts/low.sh in/x.txt", folder.display())),
        "{commands}"
    );
}

#[test]
fn a_here_source_survives_a_spitout() {
    let (tree, main) = pipeline("here-spitout", MAIN);
    let spitout = tree.path().join("main.spitout");
    let wrote = spit(&["inputs", &main, "-o", spitout.to_str().unwrap()]);
    assert!(wrote.status.success(), "{}", text(&wrote.stderr));
    let output = spit(&["dag", &main, spitout.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout)
            .matches("\"under\":\"pipeline_folder\"")
            .count(),
        4
    );
}

#[test]
fn a_here_source_is_one_file_in_the_pipeline_folder() {
    let declared = |line: &str| MAIN.replace("source lower : Script .sh here", line);
    for (main, message) in [
        (
            declared("source lower : Script .sh [id] here"),
            "line 4, column 32: a `here` source is one file",
        ),
        (
            declared("source lower : Script / here"),
            "a `here` source is one file",
        ),
        (
            declared("source lower : Script .sh here\nsource notes : Text .txt beside lower here"),
            "a `here` source is one file",
        ),
    ] {
        let (_tree, main) = pipeline("here-shape", &main);
        let stderr = text(&spit(&["check", &main]).stderr);
        assert!(stderr.contains(message), "{stderr}");
    }
    let (tree, main) = pipeline("here-missing", MAIN);
    std::fs::remove_file(tree.path().join("scripts/low.sh")).unwrap();
    let stderr = text(&spit(&["dag", &main]).stderr);
    assert!(
        stderr.contains("`here` source `lower` is not a file in the pipeline's folder"),
        "{stderr}"
    );
    let outside = MAIN.replace("path lower: scripts/low.sh", "path lower: ../low.sh");
    let (_tree, main) = pipeline("here-outside", &outside);
    let stderr = text(&spit(&["dag", &main]).stderr);
    assert!(
        stderr.contains("must not contain `.` or `..` directories"),
        "{stderr}"
    );
}

#[test]
fn a_library_outside_the_pipeline_folder_cannot_have_here_sources() {
    let tree = Tree::new(
        "here-library-outside",
        &["study/data/in/x.txt", "shared/tools/shout.sh"],
    );
    tree.write("shared/lib.spit", LIBRARY);
    let main = MAIN
        .replace("use libs/lib.spit", "use ../shared/lib.spit")
        .replace("source lower : Script .sh here\n", "")
        .replace("path lower: scripts/low.sh\n", "")
        .replace("operation low(t: Lines, script: Script) -> Lines\ncommand low: sh {script} {t} > {@output}\n", "")
        .replace("a = low(s, lower)\nb = shout(a, shouter)", "b = shout(s, shouter)");
    let main = tree.write("study/main.spit", &main);
    let stderr = text(&spit(&["dag", main.to_str().unwrap()]).stderr);
    assert!(
        stderr.contains("is `../shared/tools/shout.sh`, outside the pipeline's folder"),
        "{stderr}"
    );
}

#[test]
fn nothing_is_written_beside_a_here_source() {
    let main = format!(
        "{MAIN}operation lint(script: Script) -> (report: Text .txt, log: Text .log beside script)\ncommand lint: shellcheck {{script}} > {{report}}\npath report: out/report.txt\nreport, log = lint(lower)\n"
    );
    let (_tree, main) = pipeline("here-beside", &main);
    let stderr = text(&spit(&["dag", &main]).stderr);
    assert!(
        stderr.contains("`log` is written beside `lower`, a `here` source"),
        "{stderr}"
    );
}

#[test]
fn a_pipeline_not_read_from_a_file_has_no_here_sources() {
    let tree = Tree::new("here-no-file", &["data/in/x.txt"]);
    let main = MAIN
        .replace("use libs/lib.spit\nroot data\n", "")
        .replace("b = shout(a, shouter)\n", "");
    let pipeline = spit::parse_pipeline(&main).unwrap();
    let error = spit::discover_sources(
        &pipeline,
        &spit::InputRules::default(),
        &tree.path().join("data"),
    )
    .expect_err("no folder to find `lower` in");
    assert!(
        error
            .to_string()
            .contains("needs a pipeline read from a file"),
        "{error}"
    );
}
