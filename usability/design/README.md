# Designs and roadmap

The plans that answer the [usability findings](../FINDINGS.md), and the order to build them in.

| Plan | Resolves |
| --- | --- |
| [Removing inputs](removing-inputs.md) | F1, B1, F2, and with them F5, F7, F8 and B2 |
| [Showing each job's command line](commands-view.md) | F4 |
| [Messages that point at the cause](diagnostics.md) | B3, B5, F3, and the unmatched-file note for D5 |
| [Language changes](language.md) | F6, F9, F10 |
| [Small CLI and file fixes](small-fixes.md) | B4, B6, B7 |
| [Closing the guide's gaps](guide.md) | D1–D11 |
| [Second-study actions](round2-actions.md) | Worked examples, guide packaging, rule explanations, CLI messages, and follow-up trials |
| [One way to write each thing](one-way.md) | Aggregation, dimension order, and syntax conventions for round 3 |
| [Pilot inspection follow-ups](pre-phase5-inspection.md) | Compact job counts, collection order, and near-case unused sources |
| [Third-study inspection work](round3-inspection.md) | Job counts and diagnosis examples confirmed after Phase 5 |
| [Dataset root, output extensions and sidecars](output-paths.md) | Repeated path rules reported in every round, and `--root` on every command (proposal) |
| [Speed and cleanup after the paths work](paths-performance.md) | The audit of the paths work: `spit check` growing with products × steps, two bugs, and smells, held to the data-oriented rules |

## Status

- **Done:** steps 1–15 and 17. Phase 1, Phase 2, and Phase 3 are complete; the worked examples and extension update are in place. Step 16 was declined.
- **Second-study actions:** items 1–6 are done. The [post-Phase-5 third study](../ROUND3.md) completes the rerun.
- **Next:** implement the [round-3 inspection work](round3-inspection.md), starting with compact per-operation counts.
- **Paths:** steps 1 to 7 of [dataset root, output extensions and sidecars](output-paths.md) are done: the recipe's `root` line, extensions on operation outputs, source groups, `beside` outputs, `{x.dir}` and `{x.stem}`, `@` for built-in placeholders, and labels with optional groups.
- **Speed and cleanup:** steps 1 and 2 of [speed and cleanup after the paths work](paths-performance.md) are done: each product's producer is found once, and a rule ending in its extension after a dot in the file name is accepted. Steps 3 to 10 remain.
- **Resolved so far:** B1–B7, F1–F10, D1–D11.

## Order

The work is in four phases: small independent fixes first, then the one large change, then what builds on it.

**Rules for every step.**

- One commit per step, on the `usability` branch.
- Each commit carries its tests and updates the guide for the behaviour it changes.
- Before each commit, run `cargo test` and `usability/harness/rebuild_keys.sh`.
- A key that stops resolving to the same jobs is either a regression, or an intended change that the commit explains and re-blesses.

### Phase 1: small, independent fixes (done)

These touch separate code and change no language, so they can land in any order.

1. **B7:** the help line ([small fixes](small-fixes.md#b7-the-help-line)). Done in `b782b87`.
2. **B6:** natural order for `external_inputs` ([small fixes](small-fixes.md#b6-order-a-spitdags-lists-as-many-inputs-are-ordered)). Done in `87780cf`.
3. **F4:** `dag --commands` ([commands view](commands-view.md)). Done in `3f27d75`.
4. **B3:** file names in every message ([diagnostics](diagnostics.md#b3-name-the-file-a-message-is-about)). Done in `8e17408`.
5. **B5, F3 part 1:** count and list unused sources ([diagnostics](diagnostics.md#b5-and-f3-say-what-the-inventory-holds-that-no-job-uses)). Done in `912df53`.
6. **D1–D4, D7, D9, D10:** guide sections for behaviour that is staying ([guide](guide.md#gaps-to-write-down-now)). Done in `13801f2`.

### Phase 2: removing inputs

One design built in three steps, each leaving the tool working. See [removing inputs](removing-inputs.md).

7. **`exclude`** in all three forms, inline and from CSV. Also in this step:
   - the fixed order of the input stage;
   - the `removed:` record in the `.spitout` and `.spitdag`;
   - removal notes on stderr;
   - the unknown-statement error (B2).

   `skip` still works during this step.

   Done: `exclude` in all three forms and from CSV, the `removed:` record in the `.spitout` and `.spitdag` (with `skip` rejections), notes on stderr, the unknown-statement error, and `exclude` as the way past a stray file outside the discovered contexts (F7). The fixed order is exclusions first; `skip` keeps its two passes until `drop` replaces it.

8. **`drop` replaces `skip`.** Also in this step:
   - `require` gains `count` comparisons;
   - rule order stops mattering;
   - removing every group becomes an error;
   - a `require` with no groups left becomes an error.

   Rewrite the Recipes section of the guide (closes D8).

   Done: `drop [dims] where source` with a count, `missing` or `has` condition; `skip` is an error that shows the `drop` rule to write. Every `drop` is judged against the same inventory and their union removed at once, groups form from every artifact and context (a group with none of the target counts 0), and removing every group of a grouping is an error. `require` takes all six count comparisons, runs after the drops, and fails when its grouping finds no group. Dropped groups are notes on stderr and records in the `.spitout` and `.spitdag`, with how many the rule found. Records given directly are checked before any rule removes some. The answer keys of scenarios 2 and 6 now use `drop` and resolve to the same jobs. The Recipes section of the reference is rewritten around `exclude`, `drop` and `require` (closes D8).

9. **`dag --partial`,** the error that points to it and to `artifacts` (F5), and the hint that names an exclusion when an excluded input breaks a join. See [removing inputs, `dag --partial`](removing-inputs.md#dag---partial).
   Done: partial plans complete members of a `many` input, applies `@ min` after filtering, writes each left-out output and its gaps to the `.spitdag`, and reports the count. Plain `dag` points to `artifacts` and `--partial`, and names a matching exclusion when that caused a missing join. Scenario 6 yields 30 jobs with the summary over three stores and nine left-out artifacts.
   - **Where `dag` fails today.**
     - `prepare` in `src/main.rs` builds an `ArtifactReport` through `diagnose_checked_with_inventory` or `diagnose_checked_with_records` (`src/diagnostics.rs`).
     - In those, `record_diagnostics` returns `first_failure(&report.incomplete)` as an error unless `lenient`. `artifacts` passes `lenient = true`.
     - `--partial` can take the lenient path for `dag` too, then bind only the complete jobs: `report.dag.jobs` are complete, and `report.incomplete` lists the rest.
   - **`left_out` in the `.spitdag`.**
     - Add a `left_out` array to `BoundDag` (`src/spitdag.rs`), set in `dag()` as `removed` is.
     - Write each incomplete artifact with its reasons: the `Gap` texts `render_artifacts` prints (`src/render.rs`).
     - Document it in `docs/spitdag.md`.
   - **`many` inputs.** Check that a `many` input over partly incomplete members takes only the complete ones. The chain summary over the good stores in scenario 6 is the test.
   - **F5.** When `dag` fails without `--partial`, end the error with: "N more artifacts cannot be produced; run `spit artifacts …` to list them, or `spit dag --partial …` to plan the rest". The count is `report.incomplete` outputs.
   - **Exclusion hint.**
     - `ResolveError::MissingInput` is built in `src/resolver/matching.rs`.
     - When the missing artifact is named in `inventory.removed` (a `Removal` whose product and entities match), add a line: "`t1w[sub=02,ses=02]` was excluded by recipe line 4; exclude the session with `[sub=02,ses=02]`, or plan the rest with `--partial`".
     - The removals reach `dag` in `prepared.inputs.inventory.removed`. The hint can be added where `main.rs` reports the error, which avoids threading them through the resolver.
   - **Tests.**
     - Scenario 6's data, with its original `weekly.spitin`, planned with `--partial`. Unlike the answer key, which removes the broken stores first, it also plans every job those stores can still complete. Expect 30 jobs:
       - 16 `clean_sales`: s01, s02, s05, s07 and s09 with three weeks each, and s03's one week;
       - 10 `apply_prices`: s01, s02 and s05, and s03's week;
       - 3 `store_report`: s01, s02 and s05;
       - 1 `chain_summary` over those three reports.

       That leaves out s07's and s09's six `revenue` artifacts and the s03, s07 and s09 reports. Decide, and document, that a `many` input under `--partial` takes its complete members, so the chain summary is planned rather than left out. Compare the result with `spit artifacts` on the same data.
     - The F5 wording.
     - The exclusion hint.
     - `--partial` with no incomplete artifacts plans the same as without it.
   - **Guide.** The README's "Find incomplete artifacts" section, the reference's Recipes section (its table of rules can name `--partial` as the fourth way to leave data out), and the CLI table.

### Phase 3: messages and language

10. **F3 part 2:** near-miss hints on a failed match and on unused sources. Also the unmatched-file note and `inputs --unmatched` (closes D5). See [diagnostics](diagnostics.md#b5-and-f3-say-what-the-inventory-holds-that-no-job-uses).
    Done: missing joins show a source whose value differs only in ASCII case or leading zeros, unused near sources are warned about, and discovery counts and lists unmatched files with `inputs --unmatched`. Scenario 6's `S07.json` gets the hint while s09's absent price list does not.
    - **What exists.** `is_near` in `src/inputs/exclusions.rs` (equal ignoring ASCII case, or the same digits with different leading zeros) already powers the unmatched-`exclude` hint. Reuse it: move it somewhere shared, such as `src/model.rs` beside `natural_cmp`.
    - **Failed-match hint.** On `MissingInput` (`src/resolver/matching.rs`), look through the missing product's artifacts for one whose joined values are near. Put the hint in the error text, and in `render_artifacts` under the incomplete artifact.
    - **Unused-source warning.** An unused source (`ArtifactReport::unused_sources`, `src/model.rs`) that is near a value some incomplete job needed becomes a warning in `dag` and `artifacts`, not just part of the count.
    - **Unmatched files.** `Listing` in `src/inputs/discover.rs` walks every file. Count the files no source rule matches: add a note to `inputs`, and a `--unmatched` option that lists them on stdout and writes no `.spitout` (a new `Flag` in `src/main.rs`).
    - **Test.** Scenario 6's layout: `S07.json` gets the hint and the warning, and the store with no price list gets neither.
11. **F6:** sources without brackets, and bare names for products with no dimensions (closes D6). See [language](language.md#f6-sources-with-no-dimensions).
    Done: source declarations accept omitted brackets with or without a type; dimensionless artifact names display without `[]`, and a dimensionless source joins every driven job.
    - **Parser.** The source declaration parser is in `src/parser/declarations.rs`; the error today is "expected product name followed by [dimensions]".
    - **Display.** `push_identity` in `src/model.rs` writes `name[]` for no dimensions. Change it to write the bare name, and check that the `.spitout` reader accepts the bare record (it does for `source_lut`).
    - **Stored outputs.** Re-save and review them. The answer keys ignore display, so they must still pass.
12. **F9:** `@ vary(x, y)` with `@ drop(x, y)`. See [language](language.md#f9-aggregating-over-several-dimensions-in-one-step).
    Done: the parser accepts dimension lists, the resolver groups by the driver's remaining dimensions, the contract compares sets, and a collection keeps the source product's declared ordering. `@ min` counts the whole collection.
    - **Model.** `InputBinding::vary` and `OperationDef::aggregated_dimension` become lists (`src/model.rs`).
    - **Parsing.** Both clauses are parsed in `src/parser/declarations.rs` and `src/parser/operation.rs`.
    - **Grouping.** In `step_driver` (`src/shape.rs`), a group becomes a set difference.
    - **Checks.** The contract check in `src/compile/steps.rs` compares sets. `inferred_dimensions` in `src/lower.rs` may also need the set.
    - **Test.** The original scenario 3 shape, one leaderboard over every model and config, with collections ordered by the product's dimensions.
13. **F10:** a call's `@ vary` inferred from the operation's `@ drop`.
    Done: omitted `@ vary` inherits the operation's dropped dimensions for one or several dimensions, including flow output inference; explicit mismatches still fail, and an operation without `@ drop` still requires `@ vary`. Fill a missing `vary` from the operation in the contract check (`src/compile/steps.rs`) before checking. The lowering in `src/lower.rs` infers outputs before that, so check that it sees the filled binding.
14. **B4:** `check recipe.spitin --path-rules`, and the source wording on a pipeline. See [small fixes](small-fixes.md#b4-show-every-path-rule-whichever-file-holds-it).
    Done: recipe checks list combined pipeline and recipe path rules with origins, and strict validation uses the combined rules. A pipeline check explains that a recipe may provide a missing source rule.
    - **The refusal.** It is in `check()` in `src/main.rs`.
    - **Merging rules.** Merge the recipe's source rules with `with_source_paths` (`src/inputs/discover.rs`) before `inspect_paths`.
    - **Wording.** The `MISSING` text is in `src/paths/rules.rs`.

### Phase 4: examples and follow-ups

15. **D11:** the five worked examples, listed in [guide](guide.md#d11-worked-examples).
    Done: five small pipelines with recipes and inventories under `examples/patterns/`; the cohort has placeholder files for discovery. The reference links each example and the example test checks nested folders and expected job counts.
    - Each is an `examples/<group>/<name>/` folder with a `.spit`, `.spitin` and `.spitout`. `tests/examples.rs` checks every pipeline under `examples/`.
    - The study's answer keys are working starting points, but keep the harness's own copies unchanged.
    - Also give the existing examples' recipes data, or a note: `command_demo.spitin` finds no files beside it, and since step 8 `dag` on it fails, correctly, with "has no groups to check".
16. **Named arguments in a call:** declined. Keep positional input order and type checking against each operation port; see [language](language.md#named-arguments-in-a-call).
17. **The VS Code extension** ([spit-vscode](https://github.com/eclnz/spit-vscode)): highlight `drop`, `exclude`, `where`, `has`, `missing` and `from`; drop `skip`. Add a `file` field to `check --json` diagnostics, so the editor can place a pipeline's error found while checking a recipe ([diagnostics](diagnostics.md#b3-name-the-file-a-message-is-about)).
    Done: the grammar and semantic tokens cover the current rules, `skip` is no longer a keyword, and pipeline errors found from a recipe carry a `file` and are placed on that file in the extension.
    - **More urgent than its place suggests.** Since step 8, a recipe with `skip` is an error, and the extension still highlights `skip` as valid.
    - **Where.** The grammar is `syntaxes/spit.tmLanguage.json`: the constraint pattern near line 137 matches `(require|skip) … count(>=|=)`, and the keyword list is near line 298. `extension.js` near line 266 matches `^(?:require|skip)\s+`.
    - **Coverage.** The count pattern must accept all six comparisons, and `drop` puts its groups before the source.
    - **Tests.** The repo has `grammar.test.js` and `extension.test.js`.
    - **The `file` field.** A `Diagnostic` (`src/diagnostics.rs`) needs a place in a second file. Today the pipeline's errors inside a recipe check have no line of their own (`diagnose_recipe`).

### Phase 5: one way to write each thing

Settle the forms round 2 left open. See [one way to write each thing](one-way.md). No backward compatibility: each removed form becomes an error that shows the kept form. Each step rewrites the examples, tests, harness keys and guide it affects; the study's result folders stay as recorded.

18. **Remove the sectioned form** (`products:`, `operations:`, `pipeline:`, `commands:`, and a recipe's `constraints:`). It goes first because it halves the parser the next steps change. Convert the 8 sectioned examples and the parser tests to the flow form. See [syntax doubles](one-way.md#5-one-form-for-each-syntax-double).
    Done: the sectioned parser is gone, and each removed header is an error that says what to write instead. The eight examples are in the flow form with named ports, and the tests that exercised sections now use the flow form or were dropped where a flow test already covered them.
19. **The call says which dimensions are collected.** `@ vary` is required on every `many` input; `@ drop` on an operation is an error; `@ min` stays. See [decision 1](one-way.md#1-the-call-says-which-dimensions-are-collected).
    Done: the operation parser rejects `@ drop(...)` with the call to write, a `many` input without `@ vary` names its port, and the inference and the contract check are gone. Every pipeline, test, harness key and walkthrough now writes `@ vary` at the call; the keys resolve to the same jobs.
20. **One dimension order for the pipeline,** derived from sources, with `dimensions [...]` required only for pairs no source orders. `@ each` dimensions take their place in it, and product annotations become checks. See [decision 2](one-way.md#2-one-dimension-order-for-the-pipeline).
    Done: lowering orders every product (`src/order.rs`). Sources give the order, `dimensions [...]` declares it, and a pair nothing orders, sources that disagree, a line that leaves out or invents a dimension, and an annotation in another order are errors at their line. The ragged sweep, the reference's `each` example and both sweep keys declare `dimensions [model, config, seed]` or its equivalent; the keys resolve to the same jobs.
21. **The remaining syntax doubles:** `source x []`, the extra `many` port forms and nameless ports, positional placeholders and the `{inputs}` alias, `\{` escapes, `: path` records, and the warning for a product named like its operation. See [syntax doubles](one-way.md#5-one-form-for-each-syntax-double).
    Done: `[]` on a source, a nameless port, the `one` keyword and `many name` are errors that show the form to write, with a name suggested from the port's type; `{input}`, `{input1}` and `{inputs}` are no longer placeholders unless a port has that name; a `.spitout` record that names a file is an error; and `check` warns about a product named after its operation. Brace escapes keep Bash's rules, as below.
22. **The VS Code extension:** drop `@ drop` and the sectioned headers from the grammar, and add `dimensions`.
    Done in spit-vscode's `usability` branch (`f08893b`): the grammar follows the settled forms, an output type stops at an `@` clause, and the semantic tokens, which read the sectioned form, now color a `.spitout`'s records alone.
23. **Round 3:** rerun the scenarios with the settled language, as [second-study action 6](round2-actions.md#6-repeat-the-affected-usability-tasks) describes, keeping transcripts. Compare how alike the participants' pipelines are, not only whether the keys pass. Then decide [decision 3](one-way.md#3-the-pipeline-says-how-to-compute-the-recipe-says-which-data-after-round-3) and [decision 4](one-way.md#4-types-catch-wrong-connections-after-round-3). Done: [round 3](../ROUND3.md) archived two selected runs of each targeted task and two cohort follow-ups, all matching their keys. Participant tool transcripts were unavailable, so the access audit remains unverified.

## After the work: the second round

The [second study round](../ROUND2.md) is complete. It used the same scenarios with the new guide and these additions:

- The s2 follow-up's answer key comes from a recipe with `exclude`, not an edited `.spitout`.
- `s3-one-board` uses one leaderboard over model and config, which the old language could not express.
- `s4-vague` gives the station task with a less prescriptive brief.

**Measures to compare with round 1:**

- `spit` calls and errors per run;
- how many runs parse the `.spitdag` by hand;
- time spent on the s2 follow-up's exclusion;
- the number of guide gaps raised.

If agents misread `drop`, fall back to the name `skip` with the new grammar, as agreed.

## Working notes

For whoever picks this up next. Details of each step are in its design, linked above.

### Decisions already made

- **No backward compatibility is needed.** The language may change freely.
- **`drop` replaces `skip`.** If round 2 shows agents misreading `drop`, rename it back to `skip`, keeping the new grammar.
- **`dag` still fails by default** when a job cannot be completed; `--partial` is an opt-in flag.
- **An `exclude` that matches nothing is an error.**
- **Exclusions can come from CSV** (`exclude from file.csv`). A row's origin is its file line, `qc/excluded.csv line 3`, not a row number.
- **Later, not now:**
  - fingerprints of excluded files, to warn when a replacement appears;
  - giving `require` the same group-first shape as `drop`;
  - making `--commands` the default text view of `dag`.

### Checks before every commit

```sh
cargo fmt
cargo build --release
cargo test --release -q --no-fail-fast        # plain `cargo test` stops at the first failing test binary
cargo clippy --release --all-targets -q       # expect no output
usability/harness/rebuild_keys.sh             # expect `ok` for all seven keys
```

- **Messie.** CI also runs Messie over the repository's folders:

  ```sh
  python3 -m venv /tmp/messie-venv
  /tmp/messie-venv/bin/pip install -q -r .github/messie-requirements.txt
  /tmp/messie-venv/bin/messie .
  ```

- **Stored outputs.** They are under `tests/fixtures/outputs/`. Re-save them with `SPIT_BLESS=1 cargo test --release --test outputs`, then read `git diff tests/fixtures` before committing.
- **Links.** A changed Markdown file's links and anchors should resolve. GitHub's anchor for `## \`dag --partial\`` is `#dag---partial`.

### Conventions

- **Commit messages.** A plain sentence as the title (for example "Replace skip with drop, which names the groups it removes"), then prose saying what was wrong and what changed, then the co-author and session trailers.
- **No model names** in files pushed to the repository.
- **Guide updates travel with behaviour.** Each commit updates the README, `docs/language-reference.md`, `docs/spitdag.md` or `docs/architecture.md` for what it changes, and marks its roadmap step done here.
- **Verify before documenting.** Every claim in the guide was checked against the binary; keep doing that.

### Gotchas met so far

- **`cargo fmt` reflows code** after an edit, so a later search-and-replace on the old text can miss. Read the current text before editing.
- **The test helper `split_rules`** (`tests/support/mod.rs`) separates recipe rules from pipeline text by keyword. It knows `discover`, `require`, `drop` and `exclude`; add any new rule keyword there.
- **The input stage runs in a fixed order** (`src/inputs/mod.rs` `resolve`, and `discover` in `src/inputs/discover.rs`):
  1. find contexts and files;
  2. `exclude`;
  3. `drop`, all rules against one inventory;
  4. check expected files;
  5. `require`, in `check_inventory`, which no longer changes records.

  Records given directly are validated before any removal (`check_before_removal`).
- **Settled gaps are always reused.** `record_diagnostics` (`src/diagnostics.rs`) can do so because checking changes no records. Keep it that way, or restore the old guard.
- **`ResolvedInputs.skipped`** now holds only files whose path values cannot be read. What the rules removed is in `inventory.removed`.
- **A `Removal`** has `product`, `entities`, `rule`, `origin`, `reason` and `found`. Its `.spitout` lines are read raw, so a `#` inside a reason survives.
- **The trial sandboxes are gone.** They were in `/srv/spit-trials`, which the container that ran round 1 no longer has. `usability/harness/make_run.sh` rebuilds them; see [the study README](../README.md#run-it-again).
