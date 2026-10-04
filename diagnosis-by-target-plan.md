# Diagnosis by final target: plan

Issue eclnz/spit#59, from usability round 5, scenario s6-diagnose. Delete this
file in the last commit before the merge, as AGENTS.md says.

## What existed

Reproduced on `scenarios/s6-diagnose` (unpacked from
`usability/harness/scenarios.zip`) with `spit artifacts weekly.spitin`.

- `artifacts` printed `Incomplete artifacts: 10` flat: each incomplete output
  with its job's gaps under it. `revenue[store=s07,week=...]` appeared three
  times, each with the same reason, then `report[store=s07]` repeated all three
  as "needs revenue[...], which cannot be produced", then `summary` repeated
  each broken report.
- `IncompleteJob { outputs, gaps }` and `Gap::{Unmatched, Blocked}` in
  `src/model/dag.rs` carry everything needed; `Report` in `src/render.rs`
  prints them.
- #62 (done): the unused-source note names up to three sources.
- #67 (done, docs only): the language reference walks through
  `exclude [store=s07]` leaving `pricing[store=S07]`, with the `dag` note about
  the unused source. A failed `dag`/`artifacts` already names `pricing[store=S07]`
  as a near match for a missing input (`ResolveError::MissingInput { near }`),
  and warns that it is unused (`src/diagnostics/warnings.rs`). An `exclude`
  that matches nothing names near values (`UnmatchedExclusion::near`).
- Not existing: nothing at `inputs` time related `s07` and `S07`. After
  `exclude [store=s07]`, `inputs` showed `pricing[store=S07]` with no hint.

## Final target, in the model

There is no stored notion. An incomplete output is a final target when no
incomplete job lists it in a `Gap::Blocked`. In s6-diagnose that is `summary`
alone, because the chain summary needs every store's report. (A pipeline with
no chain step has one target per store report.) Output of an incomplete job is
per artifact, so a job with several outputs makes several nodes with the same
reasons.

## Built

### 1. `artifacts --by-target` (f8a93f5)

Flag naming follows `--partial`, `--counts`: a long kebab-case flag, with
`artifacts` accepting it. No JSON: `artifacts` has none today.
Real output from the scenario:

```text
Complete artifacts: 50

Final targets that cannot be made: 1 (incomplete artifacts: 10)
  summary : Report  (chain_summary)
    report[store=s03] : Report  (store_report)
      - input `weeks` of `store_report` needs at least 2 artifacts at [store=s03], found 1
    report[store=s07] : Report  (store_report)
      revenue[store=s07,week=2026-W36] : Revenue  (price)
        - no `pricing` artifact for input `prices` of `price` at [store=s07,week=2026-W36]
        pricing[store=S07] exists; its `store` differs only in letter case
      revenue[store=s07,week=2026-W37] : Revenue  (price)
      ...
    report[store=s09] : Report  (store_report)
      ...

Unused sources: 1
  pricing[store=S07] : Prices
```

- Each incomplete artifact that something incomplete waits on is shown once
  under its target; a second branch to it is not repeated.
- The list of complete artifacts is replaced by its count.
- Performance: render only, for the new flag only. One map from output key to
  job, one set of needed keys, an explicit stack (no recursion over chains of
  100,000 steps), report order throughout, no clones in the loops beyond the
  line text.
- Tests: `tests/artifacts.rs` (nesting, one tree per target, shown once,
  complete dataset), `tests/cli.rs` (the scenario, plain output unchanged),
  `tests/fixtures/outputs/gaps.txt` (stored output incl. a stage and a coverage gap).

### 2. Case-only variants noted at `inputs` time (0a3c827)

`inputs`, `dag` and `artifacts` (which run `inputs` in memory) print one note
per dimension, built by `case_variants` (`src/inputs/variants.rs`):

```text
note: `store` has values that differ only in letter case, which are different values to SPIT: `S07` in pricing, `s07` in sales
note: `store` has values that differ only in letter case, which are different values to SPIT: `S07` in pricing, `s07` (excluded)
```

This is the targeted hint left in #34 and #59: after a group exclusion it names
the removed group and the source filed under the other spelling. It is silent
once every spelling is removed. It does not duplicate #67: that is docs, and
the `dag` unused-source note comes later and only when `dag` gets that far.
No effect on `check --json` or `--hovers`.

## Open decisions for the maintainer

1. **Collapse sibling root causes.** The tree still repeats a cause per week
   (`revenue[store=s07,week=*]`, three entries). The tidy form is one entry per
   (job step, reason shape) with the differing dimension values listed, as
   `revenue[store=s07,week=2026-W36..W38]`. It needs a rule for what "same
   reason" means (compare `Gap` after dropping the dimension the group varies
   over), so it is a design decision, not a render detail.
2. **Should `--by-target` be the default**, or `--summary` a better name? It
   hides the complete list, so it stays opt-in here.
3. **Leading zeros** (`sub=2` and `sub=02`) are left out of the note on
   purpose (the issue says case-only). `near_reason` already treats both as
   near. Include them if wanted: a one-line change in `case_variants`.
4. **A JSON shape** for the grouped view is not built; `artifacts` has no
   `--json`, and adding one is a format decision (spit-vscode does not read
   `artifacts`).

## Suggested recipe rules (evaluated only, not built)

Two failures: a missing joined source (`no pricing artifact for ...`) and too
few artifacts (`needs at least 2 artifacts`). The natural suggestions are
`exclude [store=s09]` for both, and `exclude pricing[store=S07]` plus a rename
for the near-spelling. Risks: the fix for a missing source depends on intent
(exclude vs. get the file), for `min` it could be lowering the `@ min`, and a
suggested `exclude` hides the problem. The `--partial` plan already does the
mechanical part. A safe small piece would be to print the group exclusion that
removes every failing group, for example `exclude [store=s03]`, `[store=s07]`,
`[store=s09]`, only when failing artifacts share a dimension, as a line under
the targets. That needs a decision on which dimension to name and is left out.

## Steps

- [x] `artifacts --by-target`, docs and tests (f8a93f5)
- [x] Case-only variants note, docs and tests (0a3c827)
- [ ] Collapse sibling root causes (decision 1)
- [ ] Suggested exclusion lines (evaluated above, decision needed)
