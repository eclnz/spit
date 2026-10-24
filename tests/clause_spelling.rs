//! A clause is written `@keyword(...)`: whitespace between the `@` and its
//! keyword is an error that says what to write.

use spit::{diagnose, parse_pipeline};

const BASE: &str = "source s : T [a, b]\noperation f(x: T) -> T\n";

#[test]
fn a_space_after_the_at_of_any_clause_is_rejected_with_its_replacement() {
    for (text, line, keyword) in [
        (format!("{BASE}r = f(s @ vary(a))\n"), 3, "vary"),
        (format!("{BASE}r = f(s @ where(a=1))\n"), 3, "where"),
        (format!("{BASE}r = f(s @ same(a))\n"), 3, "same"),
        (format!("{BASE}r = f(s @ each(b))\n"), 3, "each"),
        // The second of several clauses on one input.
        (format!("{BASE}r = f(s @where(a=1) @ same(b))\n"), 3, "same"),
        (
            "source s : T [a]\noperation f(xs: many T @ min(2)) -> T\n".to_owned(),
            2,
            "min",
        ),
        (
            "source s : T [a] @ check(nonempty)\n".to_owned(),
            1,
            "check",
        ),
        (
            "source s : T [a]\noperation f(x: T @ check(nonempty)) -> T\n".to_owned(),
            2,
            "check",
        ),
        (
            "source s : T [a]\noperation f(x: T) -> (o: T @ check(nonempty))\n".to_owned(),
            2,
            "check",
        ),
    ] {
        let error = parse_pipeline(&text).unwrap_err();
        assert_eq!(error.line(), line, "{text}");
        assert_eq!(
            error.message(),
            format!("write `@{keyword}(...)`, with no space between `@` and `{keyword}`"),
            "{text}"
        );
    }
}

#[test]
fn the_compact_spelling_is_accepted_for_every_clause() {
    let text = "source s : T [a, b] @check(nonempty)\n\
                operation f(x: T @check(nonempty)) -> (o: T @check(nonempty))\n\
                operation g(xs: many T @min(2)) -> T\n\
                p = f(s @where(a=1) @same(b))\n\
                q = f(s @each(b))\n\
                r = g(s @vary(a))\n";
    parse_pipeline(text).unwrap();
}

#[test]
fn an_unknown_word_after_a_spaced_at_gets_the_error_that_lists_the_clauses() {
    let text = format!("{BASE}r = f(s @ pick(a))\n");
    let found = diagnose(&text, None);
    assert!(found[0].message.contains("`@vary(dimension)`"), "{found:?}");
}
