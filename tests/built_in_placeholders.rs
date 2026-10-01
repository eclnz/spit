//! SPIT's own path placeholders take `@`, as `{@product}`, so a reader can
//! tell them from the pipeline's dimensions, and no dimension name is
//! reserved.

use spit::{diagnose, parse_pipeline};

fn errors(text: &str) -> Vec<String> {
    diagnose(text, None)
        .into_iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| diagnostic.message)
        .collect()
}

#[test]
fn a_dimension_may_take_a_built_in_name() {
    let text = "path: out/{@stage}/{@product}/{stage}_{product}\nsource raw [stage, product]\npath raw: in/{stage}/{product}\noperation copy(a: A) -> A\nstage prep:\n    copied = copy(raw)\n";
    assert_eq!(errors(text), Vec::<String>::new());
    let pipeline = parse_pipeline(text).unwrap();
    assert_eq!(
        pipeline.path_template_for("copied").unwrap().to_string(),
        "out/{@stage}/{@product}/{stage}_{product}"
    );
}

#[test]
fn an_old_form_says_what_to_write() {
    for (name, meaning) in [
        ("product", "the product's name"),
        ("entities", "every dimension as `dimension=value`"),
        ("stage", "the stage that makes it"),
    ] {
        let text = format!(
            "path: out/{{{name}}}/{{@entities}}\nsource raw [id]\npath raw: in/{{id}}\noperation copy(a: A) -> A\ncopied = copy(raw)\n"
        );
        assert_eq!(
            errors(&text),
            [format!(
                "path template for `copied` uses absent dimension `{name}`; write `{{@{name}}}` for {meaning}"
            )]
        );
    }
}

#[test]
fn an_unknown_built_in_is_an_error() {
    let found = errors("path: out/{@name}/{@entities}\n");
    assert!(
        found.iter().any(|error| error.contains(
            "unknown built-in placeholder `{@name}`; path templates have `{@product}`, `{@entities}` and `{@stage}`"
        )),
        "{found:?}"
    );
}
