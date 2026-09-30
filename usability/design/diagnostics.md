# Design: messages that point at the cause

This plan resolves [B3](../FINDINGS.md#b3-errors-from-a-recipe-run-do-not-name-the-pipeline-file), [B5](../FINDINGS.md#b5-found-and-verified-counts-disagree-without-explanation) and [F3](../FINDINGS.md#f3-flag-unused-source-artifacts-and-near-miss-values). It also records where [B2](../FINDINGS.md#b2-an-unknown-recipe-statement-is-reported-as-a-bracket-error), [F5](../FINDINGS.md#f5-point-a-failed-dag-at-spit-artifacts) and [F8](../FINDINGS.md#f8-say-why-a-value-clause-on-a-grouping-dimension-is-rejected) are handled, since the [removing inputs](removing-inputs.md) design changes the code they touch.

## B3: name the file a message is about

**Today.** A `Diagnostic` knows whether it is about the pipeline or the inventory (`DiagnosticSource`), but not the file's name. `display_in` (`src/diagnostics.rs`) writes `line 18, column 26: …`. That is only clear when the one file on the command line is the one at fault.

Given a recipe, `dag` reports errors in the pipeline the recipe names without naming it. `check` on a recipe does name it, but through a separate path that drops the column:

```text
error: in `sweep.spit` line 7: expected product name followed by [dimensions]
```

**Change.** Every rendered diagnostic names its file whenever that file is not the one given on the command line, and keeps its column:

```text
error: pipeline.spit: line 18, column 26: no `pricing` artifact for input `prices` of `price` at [store=s07,week=2026-W36]
error: sweep.spit: line 7, column 1: expected product name followed by [dimensions]
```

- `display_in` takes the display names of the pipeline and inventory files, with `None` for the file named on the command line.
- The inventory's current prefix (`inventory line 4`) becomes its file name (`data.spitout: line 4`).
- `report` and `passed` in `src/main.rs` pass the names on, and the recipe-check path in `src/diagnostics.rs` (`in `{shown}`{line}`) switches to the same form, keeping the column.
- A name is shown as SPIT holds the path: as given on the command line, or joined to the recipe's folder for the pipeline a recipe names.
- A diagnostic about the pipeline that has no line, such as a `require` rule's coverage gap, names no file, because it is not about any line of the pipeline. An inventory is always named when its file is known.

**JSON diagnostics (moved to the VS Code step).** A `file` field on each `check --json` diagnostic needs a diagnostic to carry a place in a second file. Today, the pipeline's errors inside a recipe check have no line of their own. That is a larger change than the text form, and only the editor uses it, so it moves to the [VS Code step](README.md#phase-4-examples-and-follow-ups) of the roadmap.

**Tests.**

- Update the CLI tests in `tests/cli.rs` that match messages.
- Add `dag recipe.spitin` and `check recipe.spitin` cases for an error in the pipeline.
- Re-bless `tests/fixtures/outputs/gaps.txt` if its messages change.

## B5 and F3: say what the inventory holds that no job uses

**Today.** `inputs` and `dag` report two counts that can disagree with no explanation:

- `found 20 source artifacts under …` counts the records found (`src/main.rs`).
- `17 source files verified.` counts the sources that jobs use (`VerifiedFiles`, `src/paths/bind.rs`).

The difference is sources that no job uses. They may be left out on purpose, like calibration revisions that `where(revision=3)` filters away. They may be a mistake, like `pricing/S07.json` becoming `pricing[store=S07]` when `s07` was meant. SPIT does not tell the two apart, or mention either.

**Change 1: count unused sources.** After resolving, compute the source artifacts that no job reads: the sources in the inventory minus the external inputs of the DAG.

- `dag` and `artifacts` add a note when there are any:

  ```text
  note: 17 source files verified.
  note: 3 source artifacts are used by no job (calibration: 3); `spit artifacts` lists them
  ```

- `artifacts` lists them in a new section, after the incomplete artifacts:

  ```text
  Unused sources: 3
    calibration[station=north,revision=1]
    calibration[station=north,revision=2]
    calibration[station=south,revision=2]
  ```

**Change 2: suggest near misses.** Two artifacts are *near* when their products match and their values match on every dimension but differ as text. They count as matching on a dimension when:

- they are equal ignoring ASCII letter case (`S07` and `s07`), or
- they are equal as numbers where they are digits (`1` and `01`), which is the equality `natural_cmp` already uses in `src/model.rs`.

Two changes use this:

- **In a failed match.** When a `MissingInput` error is built (`src/resolver/matching.rs`), look among the artifacts of the missing product for one that is near the context, and add a hint:

  ```text
  error: pipeline.spit: line 18, column 26: no `pricing` artifact for input `prices` of `price` at [store=s07,week=2026-W36]
    pricing[store=S07] exists; its `store` differs only in letter case
  ```

  `artifacts` shows the same hint under the incomplete artifact.
- **Among unused sources.** An unused source that is near a value some job needed becomes a warning, not part of the plain count, since it is very likely a mistake:

  ```text
  warning: source pricing[store=S07] is used by no job; `store` differs only in letter case from s07, which `price` needs
  ```

**Cost.** The hint is only computed when a match has already failed, so correct plans pay nothing. The unused-source count is one pass over the external inputs, which the `.spitdag` already builds.

**Tests.**

- Scenario 6's layout in `tests/inputs.rs` or `tests/artifacts.rs`: one store with an upper-case price list, one with none. Check that the first gets the hint and the warning, and the second gets neither.
- A `where`-filtered source that appears in the count without a warning.
- `sub=1` against `sub=01`.

## Unmatched files (D5)

`spit inputs` skips files under the root that match no source rule, without a word. The skip is right, but four study runs could not tell it had happened. Add one note, and an option to list the files:

```text
note: 3 files under `data` match no source rule; `spit inputs --unmatched` lists them
```

`--unmatched` prints each unmatched file on stdout in place of the `.spitout`, and writes nothing. The listing already walks every file (`src/inputs/discover.rs`), so this only keeps the ones it drops.

## Handled in the removing inputs design

- **B2.** An unknown statement at the start of a recipe or pipeline line is reported as unknown, with the statements that are allowed there, not as the first syntax error inside it. The parser change is in `src/parser/flow.rs`, where top-level statements are dispatched.
- **F5.** When `dag` stops at a job it cannot complete, the error ends by counting the other artifacts that cannot be produced, and names `spit artifacts` and `spit dag --partial`.
- **F8.** The value clause of a `skip` goes away with `skip`. For `require`, the message for a value clause on a grouping dimension is reworded to explain the rule:

  ```text
  error: recipe.spitin: line 3: `store` is a grouping dimension (`per [store]`), so each group has one store; a value clause checks a dimension within each group, such as `week`
  ```
