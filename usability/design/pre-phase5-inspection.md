# Inspection ideas from the pre-Phase-5 pilot

The [pre-Phase-5 pilot](../rounds/2-pilot/README.md) showed repeated demand for a quick job count even when the complete walkthrough and `dag --commands` were available. These ideas require confirmation in the post-Phase-5 third study before changing the CLI.

## Per-operation counts

A candidate is `dag --counts` as a compact text view. It should resolve the same plan as plain `dag` and print only the total and counts in pipeline operation order:

```text
41 jobs resolved
  train: 16
  evaluate: 16
  summarise: 6
  leaderboard: 2
  compare_models: 1
```

An operation with zero jobs is omitted; a complete empty plan prints `0 jobs resolved` and no operation rows. Counts include jobs from all stages and use the operation's qualified name when imported. `--counts` accepts `--partial` and counts only planned jobs, while the existing note still reports left-out artifacts. Keep `--commands` and `--paths` for detailed inspection; `--counts` is a separate view and conflicts with them, `--json`, and `-o`. The error for a conflicting view should name both options and suggest a separate command. Add help and README entries, a test for the exact count and order on a multistage plan, a partial-plan test, and CLI conflict tests. The `.spitdag` format and answer keys should not change.

## Dimension and source inspection

The existing `dag --commands` output gives the actual `many` argument order. The pilot's walkthrough used `summary : Summary [model, config]` to produce model-first order. Phase 5 replaced that override with one pipeline-wide `dimensions [model, config, seed]` declaration; the current [sweep walkthrough](../../docs/examples.md#ragged-sweep-correlated-seeds-and-collection-order) explains it. Test that change in the final study before designing a shape-only view.

For the diagnosis case, add a short walkthrough showing `exclude [store=s07]` plus `exclude pricing[store=S07]`, with the unused source note before the second exclusion. A targeted near-case hint on an unused source after a group exclusion is worth testing, but should point to the specific artifact and rule. Do not broaden `inputs --unmatched`: the uppercase price file matches a source rule and belongs in unused-source reporting. Show `inputs --unmatched` in the guide for genuinely unmatched files.
