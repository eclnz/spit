# Design: showing each job's command line

This plan resolves [F4](../rounds/1/README.md#f4-show-each-jobs-command-line).

## The problem

The last check before running a plan is whether each job will run the command meant, with its flags and arguments in the right order. No text view shows this:

- `spit dag` prints each job's operation and artifacts.
- `spit dag --paths` adds each artifact's file.
- Neither shows the command line, or the `verify` commands that guard it.

The commands exist only in the `.spitdag`, as nested JSON lists:

```json
"command": [["fit_panel"], ["--coef"], [{"path": "build/model/coef/ne.json"}], ["--diag"], [{"path": "build/model/diag/ne.txt"}], [{"path": "build/ingest/clean/ne/wave1.csv"}], …],
"verify":  [[["validate_panel"], [{"path": "build/ingest/clean/ne/wave1.csv"}], …]]
```

Nearly every agent in the study wrote the `.spitdag` and parsed it with Python to check argument order. The survey agent called it its biggest difficulty. A swapped `--coef` and `--diag` makes a plan that resolves cleanly and overwrites the wrong file when it runs.

## The change

A new option, `spit dag --commands`, prints each job's command lines as a shell would run them.

### Output

```text
Job 12  fit_panel  [model]
  verify: validate_panel build/ingest/clean/ne/wave1.csv build/ingest/clean/ne/wave2.csv build/ingest/clean/ne/wave3.csv
  run:    fit_panel --coef build/model/coef/ne.json --diag build/model/diag/ne.txt build/ingest/clean/ne/wave1.csv build/ingest/clean/ne/wave2.csv build/ingest/clean/ne/wave3.csv

Job 13  plot_region  [publish]
  run:    plot_region build/model/coef/ne.json build/publish/chart/ne.svg
```

- **Heading.** `Job <id>  <operation>`, then the stage in brackets for a job made in a stage. A nested stage is written as `outer/inner`.
- **Checks.** One `verify:` line for each verify command, in declaration order, before the job's own command.
- **The command.** A `run:` line, or `run:    (no command)` for an operation that has none.
- **Layout.** Jobs are in plan order, separated by a blank line, as `dag` prints them now.
- **Paths.** Paths are relative to the dataset root, as in the `.spitdag`. When the root is known, stderr gets `note: commands run from <root>`, so the lines can be pasted into a shell there as a dry run.

### Quoting

Each argument is quoted the way a POSIX shell reads it, so a pasted line runs exactly the words SPIT planned:

- An argument made only of characters a shell leaves alone (letters, digits, and `_ @ % + = : , . / -`) is printed as it is.
- Any other argument is put in single quotes, with each `'` inside it written as `'\''`.
- An empty argument is printed as `''`.
- An argument joining text and a path, such as `--out=build/x.csv`, is quoted as one word.

This is the reverse of how SPIT reads a command template (`split_arguments` in `src/command.rs`). A test checks that reading a quoted line back gives the original words.

### With other options

| Options | Output |
| --- | --- |
| `--commands` | The view above |
| `--commands --paths` | The full `--paths` block for each job, followed by its `verify:` and `run:` lines |
| `--commands` with `--json` or `-o` | An error, as `--paths` is today: those write the `.spitdag` itself |

`spit dag` with no option is unchanged. Making `--commands` the default text view is a reasonable later step, since it is what most people check. It changes the output everyone sees, so it can wait until the option has been used for a while.

`artifacts` does not take `--commands`: it reports what can be made, not how.

## Implementation

**Quoting (`src/command.rs`).** Add `pub(crate) fn shell_word(text: &str) -> Cow<'_, str>`, implementing the rules above.

**Rendering (`src/render.rs`).**

- Give `render_bound_dag(dag, paths)` an options struct, `View { paths: bool, commands: bool }`.
- Add a compact writer for `--commands` alone, reusing `JobWriter` for the combined view.
- Build an argument's text from its `ArgPart`s, taking each path from `BoundDag::path`, then quote the whole word.
- `render_bound_dag` is public API (`src/lib.rs`), so keep the current signature as a wrapper, or change it in the same commit and update every caller.

**CLI (`src/main.rs`).**

- Add `Flag::Commands`, named `--commands`, accepted by `dag`.
- Help text: "show each job's command lines, as a shell would run them".
- Add `(Json, Commands)` and `(Commands, Output)` to `CONFLICTS`, and allow `Paths` with `Commands`.
- In `dag()`, bind the DAG when either `--paths` or `--commands` is given, and print the root note when the root is known.

**Tests.**

- Unit tests for `shell_word`:
  - plain words;
  - spaces;
  - a single quote;
  - `$`, a backtick, `*`, `#`, `~` and braces;
  - an empty argument;
  - non-ASCII text.
- A round-trip test: for each command in the example pipelines, split the rendered line with `split_arguments` and compare with the `.spitdag` words.
- A stored-output test in `tests/outputs.rs` for `dag --commands` and `dag --commands --paths` over the complete dataset. Write the fixtures with `SPIT_BLESS=1`.
- CLI tests in `tests/cli.rs` for the conflicts, and for `--commands` without a root.

**Documentation.**

- README: add `--commands` to the CLI table, and to the "Resolve jobs" section as the way to check a plan before writing it.
- `docs/language-reference.md`: mention it where `verify` is described, as the way to see a check in place.

**Acceptance.** Re-run scenario 5 with the harness. The logged calls should show `dag --commands`, and the transcripts should show no agent parsing the `.spitdag` to check a command.
