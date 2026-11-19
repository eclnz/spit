//! Formula validation and binding of calculations to ordinary file-producing jobs.

mod support;

use support::{spit, text, Tree};

fn pipeline(body: &str) -> (Tree, String) {
    let tree = Tree::new(
        "calculation",
        &["data/gain/01.txt", "data/reference/01.txt"],
    );
    let main = tree.write(
        "main.spit",
        &format!(
            "root data
source reference : Scalar .txt [sub]
source gain : Scalar .txt [sub]
path reference: reference/{{sub}}
path gain: gain/{{sub}}
default path: out/{{@product}}/{{sub}}
operation calibrate(reference: Scalar, exp: Scalar) -> Scalar .txt
{body}
m0 = calibrate(reference, gain)
"
        ),
    );
    (tree, main.to_str().unwrap().to_owned())
}

#[test]
fn a_calculation_emits_prefix_arrays_and_no_executable() {
    let (_tree, main) = pipeline("calculate calibrate: round(reference * exp(exp), 1)");
    let output = spit(&["dag", &main, "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let json = text(&output.stdout);
    assert!(
        json.contains(r#"{"calculate":["round",["*","reference",["exp","exp"]],1]}"#),
        "{json}"
    );
    assert!(json.contains(r#""stdout":"out/m0/01.txt""#), "{json}");
    assert!(json.contains(r#""executables":[]"#), "{json}");
    let printed = spit(&["dag", &main, "--commands"]);
    let lines = text(&printed.stdout);
    assert!(printed.status.success(), "{}", text(&printed.stderr));
    assert!(
        lines.contains(
            r#"run:    calculate ["round",["*","reference",["exp","exp"]],1] > out/m0/01.txt"#
        ),
        "{lines}"
    );

    let checked = spit(&["check", &main, "--hovers", "--json"]);
    let hovers = text(&checked.stdout);
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(
        hovers.contains("operations.md#calculating-numbers"),
        "{hovers}"
    );
}

#[test]
fn a_calculation_checks_its_formula_and_operation() {
    for (body, expected) in [
        (
            "calculate calibrate: typo + exp",
            "has no input port `typo`",
        ),
        (
            "calculate calibrate: pow(exp, 2)",
            "unknown calculation function `pow`",
        ),
        (
            "calculate calibrate: exp(exp, 2)",
            "`exp` takes 1 arguments, got 2",
        ),
        ("calculate calibrate: 2e999", "is not a finite decimal"),
        ("calculate calibrate: (exp + 2", "expected `)`"),
        (
            "calculate calibrate: exp\ncommand calibrate: echo > {@output}",
            "duplicate command for operation",
        ),
    ] {
        let (_tree, main) = pipeline(body);
        let output = spit(&["check", &main]);
        assert!(!output.status.success(), "{body}");
        let diagnostic = format!("{}{}", text(&output.stdout), text(&output.stderr));
        assert!(diagnostic.contains(expected), "{body}: {diagnostic}");
    }
}

#[test]
fn exponentiation_groups_right_and_binds_tighter_than_unary_minus() {
    let (_tree, main) = pipeline("calculate calibrate: -2^3^2 + 2^-3");
    let output = spit(&["dag", &main, "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let json = text(&output.stdout);
    assert!(
        json.contains(r#"{"calculate":["+",["neg",["^",2,["^",3,2]]],["^",2,["neg",3]]]}"#),
        "{json}"
    );
}
