//! One severity for each diagnostic, decided by the compiler: `spit check`
//! prints the diagnostics `spit check --json` gives an editor, each with the
//! same severity, and fails exactly when one is an error. A warning never
//! stops a command; an error always does.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use support::{text, Tree};

/// Each diagnostic in `check --json` output: its severity and message.
fn json_diagnostics(json: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = json;
    while let Some(at) = rest.find("\"severity\":\"") {
        rest = &rest[at + "\"severity\":\"".len()..];
        let severity = rest[..rest.find('"').unwrap()].to_owned();
        let at = rest.find("\"message\":\"").unwrap();
        rest = &rest[at + "\"message\":\"".len()..];
        let mut message = String::new();
        let mut chars = rest.char_indices();
        while let Some((index, char)) = chars.next() {
            match char {
                '"' => {
                    rest = &rest[index + 1..];
                    break;
                }
                '\\' => match chars.next().unwrap().1 {
                    'n' => message.push('\n'),
                    't' => message.push('\t'),
                    'u' => {
                        let hex: String = (0..4).map(|_| chars.next().unwrap().1).collect();
                        message
                            .push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                    }
                    escaped => message.push(escaped),
                },
                other => message.push(other),
            }
        }
        found.push((severity, message));
    }
    found
}

/// Each diagnostic `check` prints: its severity and its text after that,
/// continuation lines included.
fn printed_diagnostics(stderr: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for line in stderr.lines() {
        let started = ["error", "warning"].into_iter().find_map(|severity| {
            Some((severity, line.strip_prefix(severity)?.strip_prefix(": ")?))
        });
        match (started, found.last_mut()) {
            (Some((severity, rest)), _) => found.push((severity.to_owned(), rest.to_owned())),
            (None, Some((_, message))) => {
                message.push('\n');
                message.push_str(line);
            }
            (None, None) => panic!("`{line}` is not part of a diagnostic in:\n{stderr}"),
        }
    }
    found
}

/// Check `file` from its folder both ways, and require they agree.
/// Returns the severities found.
fn agree(file: &Path) -> Vec<String> {
    let folder = file.parent().unwrap();
    let name = file.file_name().unwrap().to_str().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_spit"))
            .args(args)
            .current_dir(folder)
            .output()
            .unwrap()
    };
    let plain = run(&["check", name]);
    let json = run(&["check", name, "--json", "--hovers"]);
    assert!(json.status.success(), "{}", file.display());
    let printed = printed_diagnostics(&text(&plain.stderr));
    let given = json_diagnostics(&text(&json.stdout));
    assert_eq!(
        printed.len(),
        given.len(),
        "{}:\n{printed:?}\n{given:?}",
        file.display()
    );
    for ((severity, shown), (json_severity, message)) in printed.iter().zip(&given) {
        assert_eq!(severity, json_severity, "{}: {message}", file.display());
        assert!(
            shown.ends_with(message.as_str()),
            "{}: `{shown}` against `{message}`",
            file.display()
        );
    }
    let errors = given.iter().any(|(severity, _)| severity == "error");
    assert_eq!(
        plain.status.success(),
        !errors,
        "{}: {printed:?}",
        file.display()
    );
    given.into_iter().map(|(severity, _)| severity).collect()
}

/// A pipeline with one of each warning `check` can give without data.
const WARNED: &str = "\
source raw [id]
path raw: in/{id}.txt
source spare [id]
stage empty:
operation clean(input)
command clean: tool {input} {@output}
operation summary(input) -> Report<T>
operation unused(input)
clean = clean(raw)
summed = summary(clean)
";

#[test]
fn check_and_its_json_give_each_diagnostic_one_severity() {
    let tree = Tree::new("severity", &["in/1.txt"]);
    tree.write("warned.spit", WARNED);
    tree.write(
        "broken.spit",
        "source raw [id, batch]\npath: {@product}/{@entities}.txt\npath raw: in/{id}.txt\noperation clean(input)\ncleaned = clean(rwa)\n",
    );
    tree.write(
        "comment.spit",
        "source raw [id]\npath raw: in/{id}.txt# not a comment\noperation clean(input)\ncleaned = clean(raw)\n",
    );
    tree.write(
        "good.spit",
        "source raw [id]\npath raw: in/{id}.txt\noperation clean(input)\ncleaned = clean(raw)\n",
    );
    tree.write("missing_root.spitin", "pipeline good.spit\nroot nowhere\n");
    tree.write(
        "bad_rule.spitin",
        "pipeline good.spit\nroot .\nrequire [id] where nothing count=1\n",
    );
    tree.write("broken_pipeline.spitin", "pipeline broken.spit\nroot .\n");
    tree.write("broken.spitout", "sources:\n    raw[id=1\n");

    let cases: &[(&str, &[&str])] = &[
        ("warned.spit", &["warning"; 6]),
        ("broken.spit", &["warning", "error", "error"]),
        ("comment.spit", &["warning"]),
        ("good.spit", &[]),
        ("missing_root.spitin", &["warning"]),
        ("bad_rule.spitin", &["error"]),
        ("broken_pipeline.spitin", &["error", "error"]),
        ("broken.spitout", &["error"]),
    ];
    for (file, severities) in cases {
        assert_eq!(&agree(&tree.path().join(file)), severities, "{file}");
    }
}

#[test]
fn every_example_checks_the_same_both_ways() {
    let mut pending = vec![PathBuf::from("examples")];
    let mut files = Vec::new();
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| {
                ["spit", "spitin", "spitout"].contains(&extension.to_str().unwrap())
            }) {
                files.push(path);
            }
        }
    }
    assert!(files.len() >= 30, "{files:?}");
    for file in files {
        agree(&fs::canonicalize(file).unwrap());
    }
}
