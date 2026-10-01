# Inspection follow-ups from round 3

The [third study](../ROUND3.md) showed repeated demand for a quick job count even when the complete walkthrough and `dag --commands` were available. This is the next CLI change to make.

## Per-operation counts

Add `dag --counts` as a compact text view. It should resolve the same plan as plain `dag` and print only the total and counts in pipeline operation order:

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

The existing `dag --commands` output gives the actual `many` argument order, and the self-contained [sweep walkthrough](../../docs/examples.md#ragged-sweep-correlated-seeds-and-collection-order) shows why `summary : Summary [model, config]` produces model-first order. Keep this as the immediate answer to the dimension request. A future shape-only view would need to show each derived product's ordered dimensions and its generating step; do not infer a new view solely from requests made before users saw the repaired guide.

For the diagnosis case, add a short walkthrough showing `exclude [store=s07]` plus `exclude pricing[store=S07]`, with the unused source note before the second exclusion. A targeted near-case hint on an unused source after a group exclusion is worth testing, but should point to the specific artifact and rule. Do not broaden `inputs --unmatched`: the uppercase price file matches a source rule and belongs in unused-source reporting. Show `inputs --unmatched` in the guide for genuinely unmatched files.
