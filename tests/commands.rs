//! Command templates are validated against their operations, with or without jobs.

use spit::{parse_pipeline, validate_commands};

#[test]
fn commands_are_validated_even_without_resolved_jobs() {
    let base = "source raw : Table [id]\noperation normalize(table: Table) -> Table\n";
    let check = |command: &str| {
        let pipeline = parse_pipeline(&format!("{base}{command}\n")).unwrap();
        validate_commands(&pipeline).map_err(|error| error.to_string())
    };
    assert!(check("command normalize: normalize --mode {table} {output}").is_ok());
    assert!(check("command normalize: normalize --mode {raw} {output}")
        .unwrap_err()
        .contains("unknown placeholder `{raw}`"));
    assert!(check("command normalize: normalize --mode {table} out.csv")
        .unwrap_err()
        .contains("must use `{output}`"));
    assert!(check("command dedupe: dedupe {input} {output}")
        .unwrap_err()
        .contains("unknown operation `dedupe`"));
}
