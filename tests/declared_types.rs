//! `type Name<Param, ...>`: a declared type takes as many arguments as it
//! has parameters, wherever it is written; under `types strict`, a type
//! written with arguments must be declared.

mod support;

use support::{errors, Tree};

use spit::{diagnose, diagnose_in, parse_pipeline, Context};

const HEADER: &str = "default entities: {@key}={@value} separated \"__\"\n\
                      default path: out/{@product}/{@entities}\n";

/// The errors of `text`, as (line, message).
fn found(text: &str) -> Vec<(Option<usize>, String)> {
    errors(diagnose(text, None))
        .into_iter()
        .map(|error| (error.line, error.message))
        .collect()
}

#[test]
fn a_declaration_names_the_parameters_in_order() {
    let pipeline = parse_pipeline("type Frame<Kind, Space>\ntype Mask<Kind>\n").unwrap();
    assert_eq!(
        pipeline.type_decls["Frame"],
        ["Kind".to_owned(), "Space".to_owned()]
    );
    assert_eq!(pipeline.type_decls["Mask"], ["Kind".to_owned()]);
}

#[test]
fn types_written_with_the_declared_arguments_pass() {
    let text = format!(
        "type Frame<Kind, Space>\ntype Space<Subject, Reference>\n{HEADER}\
         source scan : Frame<Diffusion,Space<Sub01,T1>> [sub]\n\
         operation smooth(x: Frame<S,T>) -> Frame<S,T>\n\
         command smooth: s {{x}} {{@output}}\n\
         once = smooth(scan)\n"
    );
    assert_eq!(found(&text), []);
}

#[test]
fn a_source_with_too_few_or_too_many_arguments_is_reported_where_it_is_written() {
    let text = format!(
        "type Frame<Kind, Space>\n{HEADER}source a : Frame<Native> [sub]\nsource b : Frame<A,B,C> [sub]\nsource c : Frame [sub]\n"
    );
    let said = found(&text);
    assert_eq!(said.len(), 3, "{said:?}");
    assert_eq!(said[0].0, Some(4));
    assert_eq!(
        said[0].1,
        "source `a`: `Frame<Native>` has 1 type argument, but `Frame` is declared `type Frame<Kind, Space>` and takes 2 type arguments"
    );
    assert_eq!(said[1].0, Some(5));
    assert_eq!(said[2].0, Some(6));
}

#[test]
fn a_nested_type_is_checked_in_turn() {
    let text = format!(
        "type Frame<Space>\ntype Space<Subject, Reference>\n{HEADER}source a : Frame<Space<Sub01>> [sub]\n"
    );
    let said = found(&text);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].1.contains("`Space<Sub01>` has 1 type argument"),
        "{said:?}"
    );
}

#[test]
fn an_operation_port_with_the_wrong_arguments_is_reported_at_the_operation_and_quiets_its_steps() {
    let text = format!(
        "type Frame<Kind>\n{HEADER}source a : Frame<Native> [sub]\n\
         operation smooth(x: Frame<S,T>) -> Frame<S>\n\
         command smooth: s {{x}} {{@output}}\n\
         once = smooth(a)\n"
    );
    let said = found(&text);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, Some(5));
    assert!(
        said[0].1.starts_with("operation `smooth`, port `x`:"),
        "{said:?}"
    );
}

#[test]
fn a_type_that_is_not_declared_is_not_checked_unless_strict() {
    let text =
        format!("{HEADER}source a : Frame<Native,MNI> [sub]\nsource b : Frame<Native> [sub]\n");
    assert_eq!(found(&text), []);
    let strict = format!("types strict\n{HEADER}source a : Frame<Native> [sub]\n");
    let said = found(&strict);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, Some(4));
    assert!(
        said[0].1.contains("`types strict` needs `Frame` declared"),
        "{said:?}"
    );
    assert!(said[0].1.contains("type Frame<Param>"), "{said:?}");
    // A type with no arguments needs no declaration.
    let plain = format!("types strict\n{HEADER}source a : Image [sub]\n");
    assert_eq!(found(&plain), []);
}

#[test]
fn a_bad_declaration_is_reported() {
    for (text, expected) in [
        ("type Frame\n", "expected `type Name<Param, ...>`"),
        ("type Frame<>\n", "expected type name"),
        ("type Frame<A, A>\n", "type `Frame` has parameter `A` twice"),
        ("type Frame<Space<A>>\n", "expected `type Name<Param, ...>`"),
        ("type frame<A>\n", "must start with a capital letter"),
    ] {
        let said = found(text);
        assert_eq!(said.len(), 1, "{text}: {said:?}");
        assert_eq!(said[0].0, Some(1), "{text}");
        assert!(said[0].1.contains(expected), "{text}: {said:?}");
    }
    let said = found("type Frame<A>\ntype Frame<A, B>\n");
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].0, Some(2));
    assert!(
        said[0].1.contains("already declared with other parameters"),
        "{said:?}"
    );
    // The same declaration twice is one declaration.
    assert_eq!(found("type Frame<A>\ntype Frame<A>\n"), []);
}

#[test]
fn an_import_brings_its_declarations_and_conflicts_on_different_ones() {
    let dir = Tree::new("declared_types", &[]);
    dir.write(
        "lib.spit",
        "type Frame<Kind>\noperation copy(x: Frame<S>) -> Frame<S>\ncommand copy: cp {x} {@output}\n",
    );
    let main = dir.write(
        "main.spit",
        &format!(
            "use copy from lib.spit\n{HEADER}source raw : Frame<Native,MNI> [sub]\ncopied = copy(raw)\n"
        ),
    );
    let text = std::fs::read_to_string(&main).unwrap();
    let issues = errors(diagnose_in(&text, None, Context::at(&main)));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(
        issues[0]
            .message
            .contains("`Frame<Native,MNI>` has 2 type arguments"),
        "{issues:?}"
    );

    let clash = dir.write(
        "clash.spit",
        &format!("type Frame<A, B>\nuse copy from lib.spit\n{HEADER}"),
    );
    let text = std::fs::read_to_string(&clash).unwrap();
    let issues = errors(diagnose_in(&text, None, Context::at(&clash)));
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("import conflicts with type `Frame`")),
        "{issues:?}"
    );
}
