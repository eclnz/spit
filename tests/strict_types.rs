//! `types strict`: once inference is done, every source, every port of an
//! operation the pipeline uses, and every product a step makes has a type
//! with no unknown part. Without the line, missing types are not checked.

mod support;

use support::{errors, Tree};

use spit::{diagnose, diagnose_in, parse_pipeline, validate_pipeline, Context, TypeChecking};

const HEADER: &str = "default entities: {@key}={@value} separated \"__\"\n\
                      default path: out/{@product}/{@entities}\n";

/// The types the bodies below write with arguments, declared after them so
/// that no line number moves.
const DECLARED: &str = "type Frame<Kind>\ntype Mask<Kind>\ntype Pair<A, B>\n";

/// The errors of `body` under `types strict`, as (line, message).
fn strict(body: &str) -> Vec<(Option<usize>, String)> {
    found(&format!("types strict\n{HEADER}{body}{DECLARED}"))
}

fn found(text: &str) -> Vec<(Option<usize>, String)> {
    errors(diagnose(text, None))
        .into_iter()
        .map(|error| (error.line, error.message))
        .collect()
}

#[test]
fn a_type_declared_once_and_inferred_through_generic_steps_passes() {
    let body = "source raw : Frame<Brain> [sub]\n\
                operation smooth(x: Frame<S>) -> Frame<S>\n\
                operation mask(x: Frame<S>) -> Mask<S>\n\
                command smooth: s {x} {@output}\n\
                command mask: m {x} {@output}\n\
                once = smooth(raw)\n\
                twice = smooth(once)\n\
                masked = mask(twice)\n";
    assert_eq!(strict(body), []);
    let pipeline = parse_pipeline(&format!("types strict\n{HEADER}{body}{DECLARED}")).unwrap();
    assert_eq!(pipeline.types, TypeChecking::Strict);
    assert!(validate_pipeline(&pipeline).is_ok());
}

#[test]
fn without_the_line_or_with_types_standard_missing_types_are_not_errors() {
    let body = "source raw [sub]\n\
                operation copy(x) -> .txt\n\
                command copy: cp {x} {@output}\n\
                copied = copy(raw)\n";
    assert_eq!(found(&format!("{HEADER}{body}")), []);
    assert_eq!(found(&format!("types standard\n{HEADER}{body}")), []);
    let pipeline = parse_pipeline(&format!("{HEADER}{body}")).unwrap();
    assert_eq!(pipeline.types, TypeChecking::Standard);
}

#[test]
fn an_untyped_source_is_reported_at_its_declaration_and_quiets_what_reads_it() {
    let body = "source raw [sub]\n\
                operation smooth(x: Frame<S>) -> Frame<S>\n\
                command smooth: s {x} {@output}\n\
                once = smooth(raw)\n\
                twice = smooth(once)\n";
    assert_eq!(
        strict(body),
        [(
            Some(4),
            "`types strict` needs a type on source `raw`, as in `source raw : Type [sub]`"
                .to_owned()
        )]
    );
}

#[test]
fn a_source_with_an_unknown_part_is_reported() {
    let body = "source raw : Frame<Unknown> [sub]\n";
    assert_eq!(
        strict(body),
        [(
            Some(4),
            "`types strict` needs a type with no unknown part on source `raw`, not Frame<Unknown>"
                .to_owned()
        )]
    );
}

#[test]
fn an_untyped_port_of_a_used_operation_is_reported_once_and_quiets_its_steps() {
    let body = "source raw : Image [sub]\n\
                operation copy(x, y: Image) -> .txt\n\
                operation unused(x) -> .txt\n\
                command copy: cp {x} {y} {@output}\n\
                command unused: cp {x} {@output}\n\
                copied = copy(raw, raw)\n";
    assert_eq!(
        strict(body),
        [(
            Some(5),
            "`types strict` needs a type on every port of operation `copy`, which has ports `x`, `output` without one: write `port: Type` for an input and `-> Type` for an output"
                .to_owned()
        )]
    );
}

#[test]
fn a_variable_no_input_fixes_is_reported_at_the_product_and_a_written_type_fixes_it() {
    let body = "source raw : Image [sub]\n\
                operation wrap(x: Image) -> Frame<T>\n\
                operation smooth(x: Frame<S>) -> Frame<S>\n\
                command wrap: w {x} {@output}\n\
                command smooth: s {x} {@output}\n\
                wrapped = wrap(raw)\n\
                smoothed = smooth(wrapped)\n";
    assert_eq!(
        strict(body),
        [(
            Some(9),
            "`types strict` needs a known type for `wrapped`, but `wrap` gives it Frame<$T>, and its inputs fix no `$T`; write its type, as in `wrapped : Frame<Type> [sub] = wrap(...)`"
                .to_owned()
        )]
    );
    let fixed = body.replace("wrapped = wrap", "wrapped : Frame<Brain> [sub] = wrap");
    assert_eq!(strict(&fixed), []);
}

#[test]
fn a_nested_variable_left_open_in_one_output_of_several_is_reported_at_that_output() {
    let body = "source raw : Image [sub]\n\
                operation split(x: Image) -> (a: Pair<Image,T>, b: Image)\n\
                command split: s {x} {a} {b}\n\
                first, second = split(raw)\n";
    let text = format!("types strict\n{HEADER}{body}{DECLARED}");
    let issues = errors(diagnose(&text, None));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].line, Some(7));
    assert_eq!(issues[0].columns, Some(0..5));
    assert_eq!(
        issues[0].message,
        "`types strict` needs a known type for `first`, but `split` gives it Pair<Image,$T>, and its inputs fix no `$T`; write its type, as in `first : Pair<Image,Type> [sub]` where the step names `first`"
    );
}

#[test]
fn a_call_of_an_operation_with_a_body_is_held_to_typed_ports() {
    let body = "source raw : Image [sub]\n\
                operation clean(x: Image) -> Image\n\
                command clean: c {x} {@output}\n\
                operation tidy(x) -> (result):\n    \
                    result = clean(x)\n\
                tidied = tidy(raw)\n";
    assert_eq!(
        strict(body),
        [(
            Some(7),
            "`types strict` needs a type on every port of operation `tidy`, which has ports `x`, `result` without one: write `port: Type` for an input and `-> Type` for an output"
                .to_owned()
        )]
    );
    let typed = body.replace("tidy(x) -> (result)", "tidy(x: Image) -> (result: Image)");
    assert_eq!(strict(&typed), []);
}

#[test]
fn the_importing_file_decides_and_its_setting_covers_imported_definitions() {
    let dir = Tree::new("strict_types", &[]);
    dir.write(
        "lib.spit",
        "types strict\noperation copy(x) -> .txt\ncommand copy: cp {x} {@output}\n",
    );
    let standard = dir.write(
        "standard.spit",
        &format!("use copy from lib.spit\n{HEADER}source raw [sub]\ncopied = copy(raw)\n"),
    );
    let text = std::fs::read_to_string(&standard).unwrap();
    assert!(errors(diagnose_in(&text, None, Context::at(&standard))).is_empty());
    let strict = dir.write(
        "strict.spit",
        &format!(
            "types strict\nuse copy from lib.spit\n{HEADER}source raw : Image [sub]\ncopied = copy(raw)\n"
        ),
    );
    let text = std::fs::read_to_string(&strict).unwrap();
    let issues = errors(diagnose_in(&text, None, Context::at(&strict)));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].message.contains("operation `copy`"));
}

#[test]
fn a_misplaced_or_misspelled_types_line_is_an_error() {
    assert_eq!(
        found("types loose\n"),
        [(
            Some(1),
            "expected `types strict` or `types standard`".to_owned()
        )]
    );
    assert_eq!(
        found("types strict\ntypes strict\n"),
        [(Some(2), "a pipeline has one `types` line".to_owned())]
    );
    let in_stage = found("stage prep:\n    types strict\n");
    assert_eq!(in_stage.len(), 1);
    assert!(in_stage[0]
        .1
        .contains("`types`, which covers the whole pipeline, belongs at the top level"));
}

#[test]
fn a_variable_left_open_in_a_body_is_reported_at_the_call_with_the_body_s_name() {
    let body = "source raw : Image [sub]\n\
                operation wrap(x: Image) -> Frame<T>\n\
                command wrap: w {x} {@output}\n\
                operation tidy(x: Image) -> (result: Frame<Brain>):\n    \
                    inner = wrap(x)\n    \
                    result = wrap(x)\n\
                tidied = tidy(raw)\n";
    let text = format!("types strict\n{HEADER}{body}{DECLARED}");
    let issues = errors(diagnose(&text, None));
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].line, Some(10));
    assert_eq!(issues[0].related.len(), 1);
    assert!(issues[0]
        .message
        .ends_with("write its type, as in `inner : Frame<Type> [sub] = wrap(...)`"));
}

#[test]
fn a_recipe_s_types_line_belongs_in_the_pipeline() {
    let dir = Tree::new("strict_types_recipe", &[]);
    dir.write("p.spit", "source raw [sub]\n");
    let recipe = dir.write("r.spitin", "pipeline p.spit\nroot .\ntypes strict\n");
    let output = support::spit(&["check", recipe.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(support::text(&output.stderr)
        .contains("error: line 3, column 1: `types` belongs in the .spit pipeline"));
}
