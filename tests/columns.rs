//! Each diagnostic points at the part of its line it is about.

use spit::{diagnose, DiagnosticSource};

/// Each diagnostic as `severity line: pointed-at text`.
fn pointed(text: &str, inventory: Option<&str>) -> Vec<String> {
    diagnose(text, inventory)
        .iter()
        .map(|diagnostic| {
            let source = match diagnostic.source {
                DiagnosticSource::Pipeline => text,
                DiagnosticSource::Inventory => inventory.unwrap(),
            };
            let line = diagnostic.line.unwrap();
            let columns = diagnostic.columns.clone().unwrap();
            let pointed = &source.lines().nth(line - 1).unwrap()[columns];
            format!("{} {line}: {pointed}", diagnostic.severity.as_str())
        })
        .collect()
}

#[test]
fn syntax_errors_point_at_the_offending_token() {
    let text = "\
source raw : Table [id]
  source 9bad [id]
operation copy(Table) -> Tab le
bad name = copy(raw)
operation clean(Table) -> Table
command clean: tool {input {output}
path raw: in/{id.csv
";
    assert_eq!(
        pointed(text, None),
        [
            "error 2: 9bad",
            "error 3: Tab le",
            "error 4: bad name",
            "error 6: tool {input {output}",
            "error 7: in/{id.csv",
        ]
    );
}

#[test]
fn syntax_errors_without_a_token_point_at_the_line_content() {
    let text = "source raw [id]\n    this is invalid   # trailing note\n";
    assert_eq!(pointed(text, None), ["error 2: this is invalid"]);
}

#[test]
fn step_errors_point_at_the_part_of_the_step_at_fault() {
    let text = "\
source raw : Table [id, run]
source other : Other [id]
operation clean(Table) -> Table
operation join(left: Table, right: Table) -> Table
operation merge(many Table) -> Table @ drop(run)
cleaned = clean(raw)
joined = join(cleaned, other)
typo = clean(rwa)
short = join(cleaned)
merged = merge(cleaned @ vary(batch))
";
    assert_eq!(
        pointed(text, None),
        [
            "error 7: other",
            "error 8: rwa",
            "error 9: join(cleaned)",
            // `cleaned` has no `batch` dimension to vary.
            "error 10: cleaned @ vary(batch)",
        ]
    );
}

#[test]
fn template_errors_point_at_the_placeholder_or_template() {
    let text = "\
source raw : Table [id]
operation clean(Table) -> Table
command clean: tool {input} {result}
cleaned = clean(raw)
path: {product}/{entities}.csv
path raw: in/{id}/{shard}.csv
";
    assert_eq!(
        pointed(text, None),
        ["error 3: {result}", "error 6: {shard}"]
    );
    // A rule missing one of its product's dimensions is wrong as a whole.
    let text = text.replace("raw : Table [id]", "raw : Table [id, batch]");
    assert_eq!(
        pointed(&text, None),
        ["error 3: {result}", "error 6: in/{id}/{shard}.csv"]
    );
}

#[test]
fn warnings_point_at_the_name_or_word() {
    let text = "\
source raw : Table [id]
source spare : Table [id]
operation clean(Table) -> Table
command clean: tool {input} {output}# note
cleaned = clean(raw)
";
    assert_eq!(
        pointed(text, None),
        ["warning 2: spare", "warning 4: {output}#"]
    );
}

#[test]
fn rule_and_inventory_errors_point_at_the_rule_and_record() {
    let text = "source raw [site, run]\nrequire raw count>=2 per [site]\n";
    assert_eq!(
        pointed(text, Some("sources:\n  raw[site=A,run=1]\n")),
        ["error 2: require raw count>=2 per [site]"]
    );
    assert_eq!(
        pointed(
            "source raw [id]\n",
            Some("sources:\n  raw[id=x]\n  raw[id=x]   # again\n")
        ),
        ["error 3: raw[id=x]"]
    );
}

#[test]
fn unclosed_brackets_point_from_the_opener_and_missing_arrows_at_the_type() {
    let text = "\
source raw : Table [id
operation copy(input: Table -> Table
operation tag(Table) Label
result = copy(raw
require raw count>=1 per [id
";
    assert_eq!(
        pointed(text, None),
        [
            "error 1: [id",
            "error 2: (input: Table",
            "error 3: Label",
            "error 4: (raw",
            "error 5: [id",
        ]
    );
}

#[test]
fn rule_errors_point_at_the_product_or_the_grouped_dimensions() {
    let text = "\
source raw : Table [id]
operation clean(Table) -> Table
cleaned = clean(raw)
require rwa count>=1 per [id]
require raw count>=1 per [shard]
require cleaned count=1 per [id]
";
    assert_eq!(
        pointed(text, None),
        ["error 4: rwa", "error 5: [shard]", "error 6: cleaned"]
    );
}

#[test]
fn command_errors_about_the_operation_point_at_its_name() {
    let text = "\
source raw : Table [id]
operation clean(Table) -> Table
command clean: tool {input} {output}
command cleen: tool {input} {output}
command clean: other {input} {output}
cleaned = clean(raw)
";
    assert_eq!(pointed(text, None), ["error 4: cleen", "error 5: clean"]);
}

#[test]
fn a_step_output_does_not_repeat_its_inputs_dimension_error() {
    let text = "\
source raw : Table [id, id]
operation clean(Table) -> Table
cleaned = clean(raw)
";
    assert_eq!(pointed(text, None), ["error 1: raw"]);
}

#[test]
fn display_in_counts_the_column_in_characters() {
    let text = "source raw [id]\noperation copy(one)\nx = copy(résumé)\ny = copy(rwa)\n";
    let diagnostics = diagnose(text, None);
    let shown: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.display_in(text, None).to_string())
        .collect();
    // `é` is one character, so `résumé` starts at column 10. The unknown
    // `rwa` waits until the syntax error is fixed.
    assert_eq!(
        shown,
        ["error: line 3, column 10: invalid input product `résumé`; use letters, digits, and underscores"]
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "error: line 3: invalid input product `résumé`; use letters, digits, and underscores"
    );

    let inventory = "sources:\n  raw[id=x]\n  raw[id=x]\n";
    let diagnostics = diagnose("source raw [id]\n", Some(inventory));
    assert_eq!(
        diagnostics[0]
            .display_in("source raw [id]\n", Some(inventory))
            .to_string(),
        "error: inventory line 3, column 3: duplicate source artifact `raw[id=x]`"
    );
}

#[test]
fn sectioned_steps_point_at_their_parts() {
    let text = "\
products:
    raw : Table [id]
    other : Other [id]
    result : Table [id]
operations:
    clean(Table) -> Table
pipeline:
    result = clean(other)
";
    assert_eq!(pointed(text, None), ["warning 2: raw", "error 8: other"]);
}
