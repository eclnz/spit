//! `@scalar(port, min=N, max=N)` in a command: the number in an input's file,
//! which the backend reads when the job starts.

mod support;

use support::{spit, text, Tree};

/// A pipeline whose `fit` reads `tr`'s number, with `command` as its command.
fn pipeline(name: &str, command: &str) -> (Tree, String) {
    let tree = Tree::new(name, &["data/calib/1.nii", "data/calib/2.nii"]);
    let main = tree.write(
        "main.spit",
        &format!(
            "root data
source calib : Image [sub]
path calib: calib/{{sub}}.nii
default entities: {{@key}}={{@value}} separated \"_\"
default path: out/{{@product}}/{{@entities}}
operation read_tr(image: Image) -> Scalar .txt
command read_tr: fslval {{image}} pixdim4 > {{@output}}
tr = read_tr(calib)
operation fit(tr: Scalar, calib: Image) -> Map .nii
command fit: {command}
map = fit(tr, calib)
"
        ),
    );
    (tree, main.to_str().unwrap().to_owned())
}

fn run(args: &[&str]) -> (bool, String) {
    let output = spit(args);
    let all = format!("{}{}", text(&output.stdout), text(&output.stderr));
    (output.status.success(), all)
}

#[test]
fn a_scalar_is_a_part_naming_its_file_and_bounds() {
    let (_tree, main) = pipeline(
        "scalar-part",
        "fit --tr @scalar(tr, min=5) --te=@scalar( tr , max=1e2, min=0.50 ) {calib} {@output}",
    );
    let (ok, json) = run(&["dag", &main, "--json"]);
    assert!(ok, "{json}");
    assert!(json.contains("\"version\":13,"), "{json}");
    assert!(
        json.contains(
            "[[\"fit\"],[\"--tr\"],[{\"scalar\":\"out/tr/sub=1.txt\",\"min\":5,\"max\":null}],\
[\"--te=\",{\"scalar\":\"out/tr/sub=1.txt\",\"min\":0.5,\"max\":100}]"
        ),
        "{json}"
    );
    // A scalar's program is not among the executables, and a part is not one.
    assert!(
        json.contains("\"executables\":[\"fit\",\"fslval\"]"),
        "{json}"
    );
    let (ok, commands) = run(&["dag", &main, "--commands"]);
    assert!(ok, "{commands}");
    assert!(
        commands.contains(
            "fit --tr '@scalar(out/tr/sub=1.txt, min=5)' '--te=@scalar(out/tr/sub=1.txt, min=0.5, max=100)' calib/1.nii"
        ),
        "{commands}"
    );
}

#[test]
fn a_quoted_scalar_is_text() {
    let (_tree, main) = pipeline("scalar-quoted", "fit '@scalar(tr)' {calib} {@output}");
    let (ok, json) = run(&["dag", &main, "--json"]);
    assert!(ok, "{json}");
    assert!(json.contains("[\"@scalar(tr)\"]"), "{json}");
}

#[test]
fn a_scalar_in_verify_reads_the_same_file() {
    let (tree, main) = pipeline("scalar-verify", "fit {calib} {@output}");
    let pipeline = std::fs::read_to_string(&main).unwrap().replace(
        "command fit:",
        "verify fit: test-tr @scalar(tr, min=5)\ncommand fit:",
    );
    tree.write("main.spit", &pipeline);
    let (ok, json) = run(&["dag", &main, "--json"]);
    assert!(ok, "{json}");
    assert!(
        json.contains("\"verify\":[[[\"test-tr\"],[{\"scalar\":\"out/tr/sub=1.txt\",\"min\":5,\"max\":null}]]]"),
        "{json}"
    );
}

#[test]
fn a_misused_scalar_says_what_to_write() {
    for (command, message) in [
        (
            "fit @scalar(tr {@output}",
            "line 10, column 18: command `fit`: `@scalar(` is never closed with `)`",
        ),
        (
            "fit @scalar({tr}) {@output}",
            "`@scalar({tr})` must name an input port first; write `@scalar(tr)`, with no braces",
        ),
        (
            "fit @scalar(x) {@output}",
            "`@scalar(x)`: `fit` has no input port `x`; it has `tr`, `calib`",
        ),
        (
            "fit @scalar(@output) {@output}",
            "`@scalar(@output)` must name an input port first",
        ),
        (
            "fit @scalar(tr, min=5, min=6) {@output}",
            "`@scalar(tr, min=5, min=6)` gives `min` twice",
        ),
        (
            "fit @scalar(tr, min=inf) {@output}",
            "`inf` is not a number; write a decimal such as `5` or `0.25`",
        ),
        (
            "fit @scalar(tr, min=6, max=5) {@output}",
            "`min` is more than `max`, so no value passes",
        ),
        (
            "fit @scalar(tr, low=6) {@output}",
            "`@scalar(tr, low=6)` has no `low`; its bounds are `min` and `max`",
        ),
        (
            "fit @scalar(tr, 6) {@output}",
            "`6` is not a bound; write `min=N` or `max=N`",
        ),
        (
            "@scalar(tr) {@output}",
            "a command starts with the program it runs, and `@scalar(tr)` is a number",
        ),
        (
            "fit {calib} > @scalar(tr)",
            "write the output's placeholder alone after `>`",
        ),
    ] {
        let (_tree, main) = pipeline("scalar-misuse", command);
        let (ok, output) = run(&["check", &main]);
        assert!(!ok, "{command}: {output}");
        assert!(output.contains(message), "{command}: {output}");
    }
}

#[test]
fn a_scalar_reads_one_input_file() {
    let many = "operation fit(tr: many Scalar) -> Map .nii
command fit: fit @scalar(tr) {@output}
map = fit(tr @vary(sub))";
    let output = "operation fit(tr: Scalar) -> (map: Map .nii)
command fit: fit @scalar(map) {tr} {map}
map = fit(tr)";
    for (step, message) in [
        (
            many,
            "`@scalar(tr)`: `tr` is a many input, and `@scalar` reads the number in one file",
        ),
        (
            output,
            "`@scalar(map)`: `map` is an output of `fit`, which does not exist until the command runs; `@scalar` reads an input",
        ),
    ] {
        let tree = Tree::new("scalar-ports", &["data/calib/1.nii"]);
        let main = tree.write(
            "main.spit",
            &format!(
                "root data
source calib : Image [sub]
path calib: calib/{{sub}}.nii
default path: out/{{@product}}/{{sub}}
operation read_tr(image: Image) -> Scalar .txt
command read_tr: fslval {{image}} pixdim4 > {{@output}}
tr = read_tr(calib)
{step}
"
            ),
        );
        let (ok, text) = run(&["check", main.to_str().unwrap()]);
        assert!(!ok, "{text}");
        assert!(text.contains(message), "{text}");
    }
}

#[test]
fn a_scalar_of_a_folder_is_an_error() {
    let tree = Tree::new("scalar-folder", &["data/scans/1/a.dcm"]);
    let main = tree.write(
        "main.spit",
        "root data
source scans : Dicom / [sub]
path scans: scans/{sub}
default path: out/{@product}/{sub}
operation fit(scans: Dicom) -> Map .nii
command fit: fit @scalar(scans) {@output}
map = fit(scans)
",
    );
    let (ok, text) = run(&["dag", main.to_str().unwrap(), "--json"]);
    assert!(!ok, "{text}");
    assert!(
        text.contains("`@scalar(scans)` in the command for `fit` reads the number in `scans[sub=1]`, but it is a folder, and `@scalar` reads one file"),
        "{text}"
    );
}

#[test]
fn a_check_cannot_read_a_scalar() {
    let (tree, main) = pipeline("scalar-check", "fit {calib} {@output}");
    let pipeline =
        std::fs::read_to_string(&main).unwrap() + "check big: test-big @scalar(tr) {@path}\n";
    tree.write("main.spit", &pipeline);
    let (ok, text) = run(&["check", &main]);
    assert!(!ok, "{text}");
    assert!(
        text.contains("check `big` cannot use `@scalar(tr)`: a check has no input ports"),
        "{text}"
    );
}
