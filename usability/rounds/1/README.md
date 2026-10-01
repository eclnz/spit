# Usability findings

What the [first usability study](../../README.md) found, written as a backlog: bugs, features, and gaps in the guide. Every trial produced a correct plan, so none of these stopped an agent; each is friction that cost time, forced a guess, or left an agent less sure than it should have been. Each item names the runs that showed it (under [`results.zip`](results.zip)) and, for bugs, a command that reproduces it from the repository root after `cargo build --release`.

Priority reflects how much time the item cost and how many runs hit it:

- **P1**: cost as much as a whole build, or produced a wrong plan with no error.
- **P2**: forced several runs to guess or to read the `.spitdag` JSON by hand.
- **P3**: a single run, or cosmetic.

> F1, B1 and F2 are one problem, and are resolved together by the removing inputs design (`usability/design/removing-inputs.md`), which replaces `skip` with `exclude`, `drop` and `dag --partial`. The Plan column names each item's design under `usability/design/`, and the roadmap there (`README.md`) gave the order they were built in. Those plans were deleted once built; `git log -- usability/design` finds them.

## Summary

| ID | Kind | Priority | Item | Plan |
| --- | --- | --- | --- | --- |
| [F1](#f1-exclude-individual-artifacts-in-a-recipe) | Feature | P1 | Exclude individual artifacts in a recipe | `removing-inputs.md` |
| [B1](#b1-a-skip-value-clause-reads-as-exclude-but-keeps-only-matching-groups) | Bug | P1 | A `skip` value clause reads as "exclude" but keeps only the matching groups | `removing-inputs.md` |
| [F2](#f2-record-skipped-and-excluded-groups-with-their-reason) | Feature | P1 | Record skipped and excluded groups, with their reason | `removing-inputs.md` |
| [F3](#f3-flag-unused-source-artifacts-and-near-miss-values) | Feature | P1 | Flag unused source artifacts and near-miss values | `diagnostics.md` |
| [F4](#f4-show-each-jobs-command-line) | Feature | P2 | Show each job's command line | `commands-view.md` |
| [F5](#f5-point-a-failed-dag-at-spit-artifacts) | Feature | P2 | Point a failed `dag` at `spit artifacts` | `removing-inputs.md` |
| [B2](#b2-an-unknown-recipe-statement-is-reported-as-a-bracket-error) | Bug | P2 | An unknown recipe statement is reported as a bracket error | `removing-inputs.md` |
| [B3](#b3-errors-from-a-recipe-run-do-not-name-the-pipeline-file) | Bug | P2 | Errors from a recipe run do not name the pipeline file | `diagnostics.md` |
| [B4](#b4-no-command-shows-the-full-set-of-path-rules) | Bug | P2 | No command shows the full set of path rules | `small-fixes.md` |
| [F6](#f6-accept-a-source-with-no-dimensions) | Feature | P2 | Accept a source with no dimensions, or suggest `[]` | `language.md` |
| [F7](#f7-a-stray-file-outside-discovered-contexts-should-not-stop-inputs) | Feature | P2 | A stray file outside discovered contexts should not stop `inputs` | `removing-inputs.md` |
| [F8](#f8-say-why-a-value-clause-on-a-grouping-dimension-is-rejected) | Feature | P2 | Say why a value clause on a grouping dimension is rejected | `removing-inputs.md` |
| [F9](#f9-aggregate-over-several-dimensions-in-one-step) | Feature | P2 | Aggregate over several dimensions in one step | `language.md` |
| [B5](#b5-found-and-verified-counts-disagree-without-explanation) | Bug | P3 | "Found" and "verified" counts disagree without explanation | `diagnostics.md` |
| [B6](#b6-external_inputs-is-in-text-order) | Bug | P3 | `external_inputs` is in text order | `small-fixes.md` |
| [B7](#b7-spit-help-promises-a-script) | Bug | P3 | `spit help` promises a script | `small-fixes.md` |
| [F10](#f10-smaller-language-requests) | Feature | P3 | Smaller language requests | `language.md` |
| [D1–D11](#guide-gaps) | Docs | P2 | Gaps in the guide | `guide.md` |

## Bugs

### B1. A `skip` value clause reads as "exclude" but keeps only matching groups

**P1.** Runs: s2-cohort-a and s2-cohort-b (follow-up). Asked to drop one corrupted run, the agent in run b wrote `skip bold run=3 per [sub, ses]`. `check` accepts it. At `dag` time it does the opposite of what it reads as: `skip` removes each group that *fails* the clause, so every session without a run 3 is dropped and only the session holding the corrupted run survives. In that run a second `skip sessions` rule then removed the remaining subject, leaving an empty plan, and `dag` still exited 0.

```sh
T=$(mktemp -d); cp -r usability/harness/scenarios/s2-cohort/data/. usability/harness/scenarios/s2-cohort/key/pipeline.spit "$T"
printf 'pipeline pipeline.spit\nskip bold run=3 per [sub, ses]\n' > "$T/x.spitin"
target/release/spit check "$T/x.spitin"   # Recipe valid.
target/release/spit dag "$T/x.spitin"     # skips 6 of 7 sessions; 9 jobs, all for sub-02 ses-02, run 3 included; exit 0
```

Suggested fix:

- Warn when `skip` rejects most or all groups ("`skip bold` rejected 6 of 7 groups; `skip` removes the groups that do *not* match").
- Make `dag` fail, or at least warn, when the plan resolves no jobs.
- Once [F1](#f1-exclude-individual-artifacts-in-a-recipe) exists, point to `exclude` from this warning.

### B2. An unknown recipe statement is reported as a bracket error

**P2.** Run: s2-cohort-a (follow-up). The agent guessed at an `exclude` keyword. The error pointed at a bracket, so it looked like a typo in the right statement, not a statement that does not exist.

```sh
# with $T from B1
printf 'pipeline pipeline.spit\nexclude bold[sub=02,ses=02,run=3]\n' > "$T/y.spitin"
target/release/spit check "$T/y.spitin"   # error: line 2, column 13: unclosed `[`
```

Suggested fix: when a line does not start with a known keyword, say so and list the statements a recipe accepts (`pipeline`, `discover`, `require`, `skip`, `path`, `sources:`, `contexts:`).

### B3. Errors from a recipe run do not name the pipeline file

**P2.** Runs: s6-diagnose-b, s3-sweep-b. Given a recipe, `dag` reports `error: line 18, column 26: …` for a line of the *pipeline*, without naming it, so the reader looks at line 18 of the recipe. `check` on a recipe names the file, but drops the column. Both forms appear for the same mistake:

```sh
target/release/spit dag usability/harness/scenarios/s6-diagnose/data/weekly.spitin
# error: line 18, column 26: no `pricing` artifact for input `prices` of `price` at [store=s07,week=2026-W36]
```

```text
error: line 7, column 1: expected product name followed by [dimensions]            (check pipeline.spit)
error: in `…/p.spit` line 7: expected product name followed by [dimensions]        (check recipe.spitin)
```

Suggested fix: when the file at fault is not the one on the command line, always name it, and keep the column: `error: pipeline.spit: line 18, column 26: …`.

### B4. No command shows the full set of path rules

**P2.** Run: s2-cohort-a. When a recipe supplies source path rules, `check pipeline.spit --path-rules` lists those sources as `MISSING`, which reads as an error. `check recipe.spitin --path-rules` is refused (`--path-rules and --strict-paths check a pipeline, not a recipe`), so no command shows the rules the recipe and pipeline make together.

```sh
target/release/spit check usability/harness/scenarios/s6-diagnose/data/weekly.spitin --path-rules
# error: --path-rules and --strict-paths check a pipeline, not a recipe
```

Suggested fix:

- Accept `--path-rules` on a recipe, showing each rule's origin.
- On a pipeline, write "no rule in the pipeline (a recipe may supply one)" in place of `MISSING` for a source.

### B5. "Found" and "verified" counts disagree without explanation

**P3.** Runs: s4-sensors-a and s4-sensors-b.

```sh
T2=$(mktemp -d); cp -r usability/harness/scenarios/s4-sensors/data/. usability/harness/scenarios/s4-sensors/key/*.spit* "$T2"
target/release/spit dag "$T2/dataset.spitin" -o /dev/null
# note: found 20 source artifacts under `…`
# note: 17 source files verified.
```

The 3 missing are the calibration revisions that `where(revision=3)` filtered out. Both agents worked this out, but neither was sure.

Suggested fix: add a note giving the difference: "3 source artifacts are used by no job (calibration: 3, filtered by `where(revision=3)`)". See [F3](#f3-flag-unused-source-artifacts-and-near-miss-values).

### B6. `external_inputs` is in text order

**P3.** Run: s5-survey-a. In a `.spitdag`, the waves of `external_inputs` for one region come out as `1, 10, 2`, while every `many` placeholder uses numeric order (`1, 2, 10`). This is harmless, but it made one agent doubt which order it would get. Suggested fix: sort `external_inputs` and `targets` in the same order as `many` inputs.

### B7. `spit help` promises a script

**P3.** Run: s5-survey-a. The first line of `spit help` ends "…resolve jobs, and write a script", but no command writes a script. Suggested fix: say "…and write a .spitdag".

## Features

### F1. Exclude individual artifacts in a recipe

**P1.** Runs: s2-cohort-a and s2-cohort-b (follow-up), plus the study's own answer key.

**The problem.** Asked to drop one corrupted run (the file had to stay in a read-only archive), both agents spent longer than the rest of the change request combined. For the session-model agent that was about as long as its first build. Neither found a way to do it in SPIT:

- `where(...)` only keeps values.
- `skip` removes whole groups, and inverts as described in [B1](#b1-a-skip-value-clause-reads-as-exclude-but-keeps-only-matching-groups).
- Guessed syntax was rejected, with misleading errors ([B2](#b2-an-unknown-recipe-statement-is-reported-as-a-bracket-error), [F8](#f8-say-why-a-value-clause-on-a-grouping-dimension-is-rejected)).

**The workarounds.** Both agents hand-edited a generated `.spitout`, and one wrapped this in a shell script driven by an exclusion list. Either way the exclusion lives outside SPIT: running `spit dag recipe.spitin` again, or re-running `spit inputs`, silently brings the corrupted run back. The study's own answer key had to be built the same way.

**Suggested feature.** An `exclude` rule in the `.spitin`, taking one or more artifact identities and an optional reason:

```text
exclude bold[sub=02,ses=02,run=3]    # corrupted: motion spike at volume 140
```

- It is reported on stderr like `skip`.
- It is recorded in the `.spitout` ([F2](#f2-record-skipped-and-excluded-groups-with-their-reason)), so it survives a rescan.
- It fails if it matches nothing, so a typo cannot pass silently.

A partial identity, such as `exclude bold[run=3]`, could exclude a set.

### F2. Record skipped and excluded groups, with their reason

**P1.** Runs: s2-cohort-a and s2-cohort-b, s6-diagnose-a and s6-diagnose-b.

**The problem.** A `skip` rule's result appears only as stderr lines such as `warning: skipped [sub=03] because 'skip sessions' rejected the group`. Neither the `.spitout` nor the `.spitdag` records who was left out. The brief asked for the plan to "tell us who was excluded", and both agents said the stderr line was not enough. Two more problems:

- The line is a `warning` for behaviour the user asked for.
- It does not say what was observed ("1 session; needs >=2").

**Suggested feature.**

- A `skipped:` section in the `.spitout`, and an `excluded` list in the `.spitdag`, each entry with its rule and observed count.
- The stderr line becomes a `note` that gives the count.

### F3. Flag unused source artifacts and near-miss values

**P1.** Runs: s6-diagnose-a and s6-diagnose-b.

**The problem.** One store's price list is saved as `pricing/S07.json` where the pipeline expects `pricing/s07.json`. `spit artifacts` reports s07 exactly as it reports s09, whose price list is missing: `no 'pricing' artifact for input 'prices' of 'price' at [store=s07,…]`. The misnamed file becomes `pricing[store=S07]`, a source artifact that no job uses, and nothing says so. SPIT's letter-case warning compares paths, and these belong to different identities, so it does not fire. Both agents found the cause only by listing folders by hand.

**Suggested feature.**

- Report source artifacts that no job consumes, grouped by product (this also explains [B5](#b5-found-and-verified-counts-disagree-without-explanation)).
- In a "no `X` artifact … at [dim=value]" error, name any artifact of `X` whose value differs only in case, or is otherwise close: "`pricing[store=S07]` exists; values differ only in letter case".
- Optionally, add `spit inputs --unmatched` to list files under the root that matched no source rule. Four runs asked what happens to such files.

### F4. Show each job's command line

**P2.** Runs: nearly all. Scenario 5 made it explicit.

`dag --paths` shows each job's files but not its expanded command or `verify` lines. Nearly every agent wrote the `.spitdag` and parsed its JSON with Python to check argument order.

Suggested feature: `dag --commands` prints each job's command and verify lines, quoted as a shell would show them:

```text
Job 12  fit_panel  [model]
  verify: validate_panel build/ingest/clean/nw/wave1.csv build/ingest/clean/nw/wave2.csv build/ingest/clean/nw/wave10.csv
  run:    fit_panel --coef build/model/coef/nw.json --diag build/model/diag/nw.txt build/ingest/clean/nw/wave1.csv …
```

### F5. Point a failed `dag` at `spit artifacts`

**P2.** Runs: s6-diagnose-a and s6-diagnose-b. `dag` stops at the first job it cannot complete and names only that one (store s07), which can suggest it is the only problem. Both agents knew to try `artifacts` only because the guide mentions it. Suggested fix: end the error with "N more artifacts cannot be produced; run `spit artifacts …` to list them".

### F6. Accept a source with no dimensions

**P2.** Runs: s3-sweep-a and s3-sweep-b (both), and the study's own key.

`source testset : TestSet` fails with `expected product name followed by [dimensions]`. Both agents guessed `[]`, which works, but the guide never shows it.

Suggested feature: accept a missing bracket list as no dimensions. Failing that, add "write `[]` for a source with no dimensions" to the error, and document the form (see [D6](#guide-gaps)).

### F7. A stray file outside discovered contexts should not stop `inputs`

**P2.** Run: s6-diagnose-a. With `discover stores: [store] from dirs sales/{store}`, the misnamed `pricing/S07.json` makes `spit inputs` fail outright (`source file 'pricing/S07.json' for 'pricing' lies outside the discovered contexts`). The data could not be renamed, so the agent had to abandon `discover`.

```sh
T3=$(mktemp -d); cp -r usability/harness/scenarios/s6-diagnose/data/. "$T3"
printf 'pipeline pipeline.spit\ndiscover stores: [store] from dirs sales/{store}\n' > "$T3/w.spitin"
target/release/spit inputs "$T3/w.spitin"
```

Suggested feature: report such files as a warning and leave them out, or let a recipe choose between the two with a rule. Either way, list every such file, not just the first.

### F8. Say why a value clause on a grouping dimension is rejected

**P2.** Runs: s6-diagnose-b, s2-cohort-a (follow-up). Agents tried to name the groups to leave out, with `skip sales store=s07 per [store]` or `skip bold run=1,2 per [sub, ses, run]`. The error did not explain the problem to either of them:

```text
error: coverage rule for `sales` requires values of `store`, which must be a dimension of that product outside its groups
```

Suggested fix: explain the rule in the user's terms: "`store` is in `per [store]`, so each group has one store; a value clause checks a dimension within each group, such as `week`". When the intent is plainly to drop named groups, point to `exclude` ([F1](#f1-exclude-individual-artifacts-in-a-recipe)).

### F9. Aggregate over several dimensions in one step

**P2.** Found while designing scenario 3: a leaderboard over every model and config needs one step that collects over two dimensions. `@ vary(model, config)` fails with "takes one dimension", `@ vary(model) @ vary(config)` with "duplicate `@ vary(...)`", and `@ drop(model, config)` with "invalid aggregated dimension". The scenario was rewritten as two levels of rollup so that it could be solved.

```sh
printf 'source a : A [x, y]\noperation f(items: many A) -> A @ drop(x) @ drop(y)\ncommand f: f {items} {output}\nb = f(a @ vary(x) @ vary(y))\n' > /tmp/v.spit
target/release/spit check /tmp/v.spit
```

Suggested feature: allow `@ vary(x, y)` with `@ drop(x, y)`, ordering the collected artifacts by the product's dimensions as for one dimension.

### F10. Smaller language requests

**P3.**

- **Call arguments follow port order even though the driving input may sit anywhere** (s3-sweep-b). An agent put the driving input first and got a type mismatch at another port. Keyword arguments, such as `train(seed: seedfile, model: model @ each(model), …)`, would remove the mismatch. So would stating the rule beside the driving-input rule.
- **`@ drop(x)` on the operation must repeat `@ vary(x)` on the call** (s1-logs-a, s1-logs-b). Inferring one from the other when only one call uses the operation would remove a line that must be kept in step.
- **An `each` dimension is placed last** in the product's dimensions (`[config, seed, model]`, s3-sweep-b). This matters for `{entities}` and should be documented.

## Guide gaps

These are things agents looked for in the guide and did not find. The count is how many of the 12 first builds raised each one.

| ID | Runs | Gap | Where it belongs |
| --- | --- | --- | --- | --- |
| D1 | ~8 | A recipe may be just its `pipeline` line, with path rules finding the sources. Every recipe example has `discover` or `require`, so most agents wondered whether a rule was required. | Recipes; Supply the inputs |
| D2 | 5 | What paths are relative to, and where to put the recipe relative to the data. The CLI table describes `--root` on `dag` as only a check that files exist, but with a recipe it also sets the folder scanned and the base of every path. One agent put its recipe inside `data/` with `pipeline ../rest.spit`, unsure `..` was allowed. | CLI options; Paths; Recipes |
| D3 | 6 | The `.spitdag` format: how a command's words and paths are encoded, `root` (written as an absolute path), `external_inputs`, `targets`, `executables`, `fingerprint`, `stage` and `verify`. | Resolve jobs; a new reference section |
| D4 | 5 | How `many` inputs order text values: ISO dates, names such as `lr-high`, mixed values such as `run-2`, and leading zeros (`01` against `1`). The guide says only that numbers compare as numbers. | Operations and commands |
| D5 | 4 | A path rule matches the whole path, so `x.csv.bak` and `x.log.1` are ignored, and files that match no rule are skipped silently. | Paths |
| D6 | 3 | A source with no dimensions (`source x : T []`), that such an input matches every job, a fixed path for a product with no dimensions, and why it prints as `name[]`. | Products and dimensions; Paths |
| D7 | 1 | A source path rule may be written in the pipeline or the recipe, not both, and which is preferred. One agent learnt it from `source 'log' has path rules in both .spit and .spitin`. | Recipes |
| D8 | 2 | `skip` and `require` work on source products, not only on discoveries. Groups are formed across sources, and a group with none of the counted product counts as 0: this is what lets `skip pricing count=1 per [store]` drop stores with no price list. | Constraints |
| D9 | 2 | What `verify` means for a backend: a failure stops the job, then what happens to its dependents, and how it appears in the `.spitdag`. | Operations and commands |
| D10 | 3 | Whether operations, products and dimensions share a namespace (`model @ each(model)`, a product named like its operation), and how to write an untyped `many` port. | Operations and commands |
| D11 | 5 | Worked examples that agents asked for by name. | Examples |

The worked examples requested for D11:

- Picking one revision from an archive (`where`).
- One file per group, named differently in each (`same`).
- A model fit: a `many` input, two outputs, `@ drop` and `verify` together.
- A cross product with a ragged inner dimension (`each`).
- A BIDS dataset with `skip`.

## What worked

To keep in mind when changing things:

- Irregular data needed no special cases in any scenario: a missing day, an extra run, uneven seed sets, a non-contiguous wave, a station with no reading for one date.
- Adding a subject needed no edits: 1–2 minutes, against 15–20 for the first build. Adding a per-session QC step took four lines, which checked first time.
- `spit artifacts` gave both diagnosis agents the whole cause tree in one command.
- Agents called the error messages accurate and well located, and the `note:` counts useful checks.
- Compared with a shell script or Snakemake, agents rated SPIT about as fast to write and more trustworthy to check. Their main cost was reading the 470-line guide.
