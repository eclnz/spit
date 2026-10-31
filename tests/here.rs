//! `{@here}` in a command: the folder of the file that declares the operation.

mod support;

use support::{spit, text, Tree};

const LIBRARY: &str =
    "operation shout(t: Lines) -> Lines\ncommand shout: sh {@here}/shout.sh {t} > {@output}\n";

const MAIN: &str = "use shout from libs/lib.spit
root data
source s : Lines [id]
path s: in/{id}.txt
operation low(t: Lines) -> Lines
command low: sh {@here}/scripts/low.sh {t} > {@output}
default entities: {@key}={@value} separated \"_\"
default path: out/{@product}/{@entities}.txt
a = low(s)
b = shout(a)
";

fn pipeline(name: &str) -> (Tree, String) {
    let tree = Tree::new(name, &["data/in/x.txt"]);
    tree.write("libs/lib.spit", LIBRARY);
    let main = tree.write("main.spit", MAIN);
    (tree, main.to_str().unwrap().to_owned())
}

fn dag(main: &str, flag: &str) -> String {
    let output = spit(&["dag", main, flag]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
}

#[test]
fn here_is_the_folder_of_the_file_declaring_the_operation() {
    let (tree, main) = pipeline("here-folders");
    let folder = std::fs::canonicalize(tree.path()).unwrap();
    let folder = folder.display().to_string();
    let json = dag(&main, "--json");
    assert!(
        json.contains(&format!("\"pipeline_folder\":\"{folder}\"")),
        "{json}"
    );
    assert!(
        json.contains("[{\"here\":\"\"},\"/scripts/low.sh\"]"),
        "{json}"
    );
    assert!(
        json.contains("[{\"here\":\"libs\"},\"/shout.sh\"]"),
        "{json}"
    );
    let commands = dag(&main, "--commands");
    assert!(
        commands.contains(&format!("sh {folder}/scripts/low.sh in/x.txt")),
        "{commands}"
    );
    assert!(
        commands.contains(&format!("sh {folder}/libs/shout.sh out/a/id=x.txt")),
        "{commands}"
    );
}

#[test]
fn moving_the_pipeline_changes_no_fingerprint() {
    let (_first, main) = pipeline("here-first");
    let (_second, moved) = pipeline("here-second");
    let fingerprints = |json: &str| {
        json.match_indices("\"fingerprint\":")
            .map(|(at, _)| json[at..at + 32].to_owned())
            .collect::<Vec<_>>()
    };
    let (first, second) = (dag(&main, "--json"), dag(&moved, "--json"));
    assert_eq!(fingerprints(&first).len(), 2);
    assert_eq!(fingerprints(&first), fingerprints(&second));
}

#[test]
fn here_gives_no_dir_or_stem_and_is_no_redirection() {
    for (command, message) in [
        (
            "sh {@here.dir}/a.sh {t} > {@output}",
            "gives no `.dir` or `.stem`",
        ),
        ("sh a.sh {t} > {@here}", "`{@here}` is a folder, not a file"),
    ] {
        let tree = Tree::new("here-misuse", &["data/in/x.txt"]);
        let text_ = format!(
            "root data\nsource s : Lines [id]\npath s: in/{{id}}.txt\noperation low(t: Lines) -> Lines\ncommand low: {command}\ndefault path: out/{{@product}}.txt\na = low(s)\n"
        );
        let main = tree.write("main.spit", &text_);
        let output = spit(&["check", main.to_str().unwrap()]);
        assert!(!output.status.success());
        let stderr = format!("{}{}", text(&output.stderr), text(&output.stdout));
        assert!(stderr.contains(message), "{stderr}");
    }
}
