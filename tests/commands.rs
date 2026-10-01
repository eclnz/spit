//! Command templates are validated against their operations, with or without jobs.

use spit::{parse_pipeline, validate_commands};

#[test]
fn commands_are_validated_even_without_resolved_jobs() {
    let base = "source raw : Table [id]\noperation normalize(table: Table) -> Table\n";
    let check = |command: &str| {
        let pipeline = parse_pipeline(&format!("{base}{command}\n")).unwrap();
        validate_commands(&pipeline).map_err(|error| error.to_string())
    };
    assert!(check("command normalize: normalize --mode {table} {@output}").is_ok());
    assert!(
        check("command normalize: normalize --mode {table} {output}")
            .unwrap_err()
            .contains("`{output}` is now `{@output}`")
    );
    assert!(check("command normalize: normalize --mode {table} {@output.dir}").is_ok());
    assert!(
        check("command normalize: normalize --mode {table} {output.dir}")
            .unwrap_err()
            .contains("`{output}` is now `{@output}`")
    );
    assert!(check("command normalize: normalize --mode {raw} {@output}")
        .unwrap_err()
        .contains("unknown placeholder `{raw}`"));
    assert!(check("command normalize: normalize --mode {table} out.csv")
        .unwrap_err()
        .contains("must use `{@output}`"));
    assert!(check("command dedupe: dedupe {input} {@output}")
        .unwrap_err()
        .contains("unknown operation `dedupe`"));
}

#[test]
fn unnamed_output_placeholder_does_not_name_a_multi_output_port() {
    let pipeline = parse_pipeline(
        "operation split(table: Table) -> (left: Table, right: Table)\ncommand split: split {table} {left} {right} {@output}\n",
    )
    .unwrap();
    let error = validate_commands(&pipeline).unwrap_err();
    assert!(error
        .to_string()
        .contains("unknown placeholder `{@output}`"));
}
