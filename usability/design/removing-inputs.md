# Design: removing inputs

This design replaces `skip` with two rules and a flag, and records every removal. It resolves [F1](../FINDINGS.md#f1-exclude-individual-artifacts-in-a-recipe), [B1](../FINDINGS.md#b1-a-skip-value-clause-reads-as-exclude-but-keeps-only-matching-groups) and [F2](../FINDINGS.md#f2-record-skipped-and-excluded-groups-with-their-reason), which are one problem: `skip` was never scoped. There is no need to keep existing recipes working.

## Why `skip` has to go

A `skip` rule is a `require` rule with its action flipped: the same `CoverageRule`, the same grammar, and the same group logic (`src/model.rs`, `src/inputs/coverage.rs`). Everything the study hit follows from that:

- **It reads backwards.** The clause states what a group must have to be *kept*, but the verb says remove. So `skip bold run=3 per [sub, ses]` removes every session that *lacks* run 3 ([B1](../FINDINGS.md#b1-a-skip-value-clause-reads-as-exclude-but-keeps-only-matching-groups)).
- **It can only remove a group that fails a count or presence test.** A value clause may not name a grouping dimension, so no named group and no single artifact can be removed ([F1](../FINDINGS.md#f1-exclude-individual-artifacts-in-a-recipe)).
- **Its result depends on rule order.** Rules apply one after another, each changing the inventory the next sees, and during discovery they run in two passes.
- **Its groups depend on the target.** A rule on a discovery groups only that discovery's contexts; a rule on a product groups every binding in the inventory.
- **Its removals are recorded nowhere** but stderr warnings ([F2](../FINDINGS.md#f2-record-skipped-and-excluded-groups-with-their-reason)).
- **It can make a `require` pass vacuously.** When `skip` removes every group, a following `require … count>=1` checks nothing and passes (`tests/cli.rs`).

The study showed four separate intents behind "leave this out", and `skip` fitted only the first:

| Intent | Example | Rule in this design |
| --- | --- | --- |
| Cohort: remove groups that fail a criterion, which changes as the data changes | Subjects with fewer than two sessions | `drop` |
| Curation: remove a named artifact or group, with a reason, surviving a rescan | A corrupted run; a store left out until fixed | `exclude` |
| Partial plan: plan what can be completed, leave out what cannot | This week's stores with problems | `dag --partial` |
| Gate: fail if the data is incomplete | Every session has one T1w | `require` (kept) |

## Recipe rules

### `exclude`: remove named artifacts

```text
exclude bold[sub=02,ses=02,run=3]    # corrupted: motion spike at volume 140
exclude [store=s07]                  # price list misnamed; left out until fixed
exclude bold[run=3]                  # run 3 dropped from the protocol
exclude from qc/excluded.csv
```

**One matching rule.** An exclude names an optional product and some dimension values. It removes every source artifact whose identity includes all of those values, and every discovered context that does. There are three forms:

- **One exact artifact**, `bold[sub=02,ses=02,run=3]`: a product with all its dimensions.
- **A group across products**, `[store=s07]` or `[sub=02,ses=02]`: no product, some dimensions. It removes every product's artifacts, and every discovered context, whose identity includes those values.
- **Part of one product**, `bold[run=3]` or `bold[sub=02]`: a product with some of its dimensions. It removes only that product's artifacts. Artifacts of other products stay, and so does any work they drive.

**Reasons.** A comment on the line is recorded as the reason. It is optional.

**Must match.** Each exclude, and each row of a file, must match at least one artifact or context, or `inputs` fails. This catches typos such as `sub=2` for `sub=02`, and stale exclusions left behind after the data changed:

```text
error: recipe.spitin: line 4: `exclude bold[sub=2,ses=02,run=3]` matches nothing; `bold` has sub=02
```

**Checks without data.** `spit check` tests each exclude against the pipeline:

- the product exists and is a source;
- each dimension belongs to that product, or, for a group, to some source.

**From a file.** `exclude from <file>.csv` reads identities from a CSV file, relative to the recipe's folder, so a lab can keep its QC decisions in a spreadsheet:

```csv
product,sub,ses,run,reason
bold,02,02,3,motion spike at volume 140
,03,,,withdrew consent
```

- The header names the columns. `product` and `reason` are optional; every other column is a dimension, and must belong to some source.
- Each row is one exclude. An empty cell leaves that dimension unconstrained, so an empty `product` makes the row a group form.
- Every row must match something. An error names the file and row: `qc/excluded.csv: row 3: …`.
- Fields follow RFC 4180 (quotes, doubled quotes, commas inside quotes). SPIT has no CSV dependency today, and this subset is small enough to parse by hand.

### `drop`: remove groups that fail a criterion

```text
drop [sub] where sessions count<2
drop [sub, ses] where t1w count=0
drop [sub, ses] where bold missing run=1,2
drop [sub, ses] where bold has run=3
```

**Shape.** The group comes first, then `where`, then the condition for *removal*, so the rule reads the way it acts:

- **Group:** `[dims]` is the grouping. Every dimension must belong to the target.
- **Target:** a source product, or a discovery rule's name.
- **Condition:** one of three kinds.
  - `count` with any of `=`, `!=`, `<`, `<=`, `>`, `>=`, counting the target's artifacts, or discovered contexts, in the group.
  - `missing dim=v,…`: the group lacks at least one listed value.
  - `has dim=v,…`: the group holds at least one listed value.

**Several rules.** Each `drop` line is one condition, and a group is removed if any rule's condition holds. There is no `and` or `or` within a line.

**Every group, whatever the target.** A group is each distinct value of `[dims]` found anywhere in the curated inventory: in any product's artifacts and in any discovered context. A group with none of the target counts 0. This makes `drop [store] where pricing count=0` mean what it says, which today's `skip pricing count=1` only does by accident.

**Order does not matter.** Every `drop` is judged against the same inventory, after `exclude`, and the union of what they reject is removed at once. No rule sees another's result.

**Removing everything is an error.** When `drop` rules remove every group of a grouping:

```text
error: drop rules removed all 4 [sub] groups; nothing is left to plan
```

### `require`: unchanged in meaning

`require` keeps its grammar and meaning: it fails the run. It gains the same `count` comparisons as `drop`, and it is checked last, against what `exclude` and `drop` leave. A `require` whose grouping has no groups left fails, rather than passing over nothing:

```text
error: recipe.spitin: line 7: `require t1w count=1 per [sub, ses]` has no [sub, ses] groups to check
```

The grammars of `drop` (group first) and `require` (target first) now differ. Aligning them, for example as `require [sub, ses] where t1w count=1`, is left open, because `require t1w count=1 per [sub, ses]` already reads correctly.

## Order of the input stage

`spit inputs`, and `dag` or `artifacts` given a recipe, settle a dataset in one fixed order:

1. **Find.** Match discovered directories and source files. Collect the files each discovered context expects but lacks as *missing*; do not fail on them yet.
2. **Exclude.** Remove what each `exclude` names. Fail on any exclude that matched nothing.
3. **Drop.** Judge every `drop` against the result of step 2 and remove the union. A missing expected file counts as absent, so `drop [sub, ses] where t1w count=0` removes a session whose T1w is missing, instead of failing on it.
4. **Check files.** Fail on any expected file still missing.
5. **Require.** Check every `require` against what is left.
6. **Record.** Write the inventory and what was removed.

This replaces today's two passes of `skip` in `src/inputs/discover.rs`. It also gives [F7](../FINDINGS.md#f7-a-stray-file-outside-discovered-contexts-should-not-stop-inputs) a way out: a stray file outside every discovered context is an error only after step 2, so `exclude pricing[store=S07]` can name it.

### What removal removes

- **A group:** removing a group removes every artifact and discovered context whose identity lies in it, across products.
- **Coarser artifacts:** an artifact without all the group's dimensions stays, such as a subject's reference when only its sessions are dropped. If no job then uses it, it is reported as an unused source ([F3](../FINDINGS.md#f3-flag-unused-source-artifacts-and-near-miss-values)).
- **A joined input:** excluding an input that another input is joined to makes the dependent jobs incomplete, not absent. For example, excluding `t1w[sub=02,ses=02]` leaves that session's runs driving `coreg` with nothing to join. `dag` then fails by default, and its error names the exclusion:

```text
error: pipeline.spit: line 12, column 9: no `t1w` artifact for input `ref` of `coreg` at [sub=02,ses=02,run=1]
  t1w[sub=02,ses=02] was excluded by recipe.spitin line 4; exclude the session with `[sub=02,ses=02]`, or plan the rest with `--partial`
```

## Recording what was removed

**In the `.spitout`.** A `removed:` section lists each removal with the rule that made it:

```text
removed:
    bold[sub=02,ses=02,run=3]: exclude, recipe.spitin line 4  # corrupted: motion spike at volume 140
    bold[sub=05,ses=01,run=2]: exclude, qc/excluded.csv row 2  # motion spike
    [sub=03]: drop [sub] where sessions count<2, recipe.spitin line 6 (found 1)
```

- The section is a record, not a rule: resolving a `.spitout` removes nothing more, and a person writing an inventory may leave it out.
- Scanning again rewrites it from the recipe, so an exclusion no longer disappears on rescan.

**In the `.spitdag`.** A top-level `removed` array holds one object per entry:

- `identity`, or `group`;
- `rule`: its text;
- `origin`: file and line or row;
- `reason`;
- `found`, for a count.

**On stderr.** Each removal is a `note`, not a `warning`, since the user asked for it, and there is one summary line: `note: removed 1 artifact and 1 group; see removed: in the .spitout`.

## `dag --partial`

`dag` keeps failing at a job it cannot complete. Its error now ends by pointing at the next step ([F5](../FINDINGS.md#f5-point-a-failed-dag-at-spit-artifacts)):

```text
error: pipeline.spit: line 18, column 26: no `pricing` artifact for input `prices` of `price` at [store=s07,week=2026-W36]
  5 more artifacts cannot be produced; run `spit artifacts` to list them, or `spit dag --partial` to plan the rest
```

`spit dag --partial` plans every artifact that `spit artifacts` would call complete, and leaves out the rest:

- A `many` input takes the members that can be completed, so the chain summary covers the stores that have reports.
- The `.spitdag` gains a `left_out` array giving each artifact left out and why, in the same terms as `artifacts`.
- stderr gets a summary: `note: planned 22 jobs; left out 10 artifacts that cannot be produced (see left_out)`.
- The exit code is 0.

`--partial` is a choice made for one run. It is not a rule in the recipe, because what can be completed follows from the pipeline, and restating it as rules (scenario 6's `skip sales count>=2`, a copy of `@ min(2)`) drifts from the pipeline.

## The study's cases under this design

| Case | Today | With this design |
| --- | --- | --- |
| s2: subjects with one session | `skip sessions count>=2 per [sub]` | `drop [sub] where sessions count<2` |
| s2 follow-up: corrupted run | Hand-edited `.spitout`, lost on rescan | `exclude bold[sub=02,ses=02,run=3]  # corrupted` |
| s6: stores with problems | Two `skip` rules that restate the pipeline | `spit dag --partial`, or `exclude [store=s03]` and so on, while they are fixed |
| s6 with `discover`: stray `S07.json` | Hard error, no way out | `exclude pricing[store=S07]  # misnamed copy of s07` |
| B1: `skip bold run=3 per [sub, ses]` | Keeps only sessions with run 3 | `drop [sub, ses] where bold has run=3` removes them; `exclude bold[run=3]` removes the runs |

## Implementation notes

**Model.**

- Replace `CoverageRule` and `CoverageAction` (`src/model.rs`) with separate `Exclude`, `Drop` and `Require` rules in `InputRules`.
- An `Exclude` holds a pattern (an optional product and dimension values), a reason and an origin (a recipe line, or a CSV file and row).
- A `Drop` holds a grouping, a target and a condition.
- Both `Drop` and `Require` gain `Count` comparators.

**Parser.**

- In `src/parser/keyword.rs`, replace `skip` with `drop` and `exclude`.
- In `src/parser/declarations.rs` and `src/parser/flow.rs`, parse the new forms and `exclude from`.
- An unknown statement at the start of a line gets its own error, listing the statements a recipe accepts ([B2](../FINDINGS.md#b2-an-unknown-recipe-statement-is-reported-as-a-bracket-error)).
- The `.spitout` parser (`src/parser/inventory.rs`) reads and keeps a `removed:` section.

**Input stage.**

- Rewrite `src/inputs/coverage.rs` around the six steps above, with one definition of a group for `drop` and `require`, which keeps reusing `SkipIndex` for removal.
- Collapse the two passes of `skip` in `src/inputs/discover.rs` into steps 1–4.
- `ResolvedInputs.skipped` (`src/inputs/mod.rs`) becomes structured `removed` entries, not strings.

**Output.**

- Write `removed` to the `.spitout` and `.spitdag` (`src/spitdag.rs`).
- Replace the `warning: skipped …` lines in `src/main.rs` with notes.

**Resolver.**

- Carry the set of excluded identities into resolution, so a missing input can name the exclusion that caused it.
- `--partial` reuses the complete set that `artifacts` already computes.

**Tests.**

- Replace the `skip` cases in `tests/cli.rs`, `tests/discovery.rs`, `tests/inputs.rs`, `tests/outputs.rs`, `tests/parser.rs` and `tests/scaling.rs`.
- Turn the vacuous-`require` case in `tests/cli.rs` into the new error.
- Add a test for each error above, and for rule order not mattering.

**Guide.**

- Rewrite the Recipes section of `docs/language-reference.md` around the four intents, and update the README and `docs/architecture.md`.
- Document [D8](../FINDINGS.md#guide-gaps) as part of this: how groups are formed, and how a group with none of the target counts.

**Acceptance.**

- Re-run the s2 follow-up and s6 scenarios in `usability/harness`. Update their keys only if the jobs are meant to change; the s2 follow-up key should now be reachable from a recipe alone.
- If `drop` reads badly to the agents, fall back to the name `skip` with this grammar.

## Later

- **Fingerprints.** An exclude could record the excluded file's fingerprint and warn when a different file appears at that path, to catch an exclusion that outlived its reason.
- **Aligned grammar.** `require` could take the same group-first shape as `drop`.
