# Inspection work confirmed by round 3

The [post-Phase-5 study](../ROUND3.md) repeated the request for concise job counts. Implement `dag --counts` as a compact text view that resolves the same plan as plain `dag` and prints the total and counts in pipeline operation order:

```text
41 jobs resolved
  train: 16
  evaluate: 16
  summarise: 6
  leaderboard: 2
  compare_models: 1
```

Omit operations with zero jobs; for an empty plan print `0 jobs resolved` with no rows. Count jobs across all stages and use qualified operation names for imports. With `--partial`, count only planned jobs and keep the existing left-out note. Treat `--counts` as a separate view that conflicts with `--commands`, `--paths`, `--json`, and `-o`; the conflict message should suggest running the views separately. Add CLI help and README entries, an exact count and order test, a partial-plan test, and conflict tests. The `.spitdag` and answer keys should remain the same.

`dag --commands` already shows the actual order of a `many` input. The [sweep walkthrough](../../docs/examples.md#ragged-sweep-correlated-seeds-and-collection-order) now shows the pipeline-wide `dimensions` line that controls that order. A shape-only view is not needed to address the passing sweep runs.

For the diagnosis example, show `exclude [store=s07]` and `exclude pricing[store=S07]` together, with the unused source note before the second rule. A future targeted hint could name the orphan source and the near-case removed group. Keep `inputs --unmatched` for files that match no source rule; the uppercase pricing file matches a source rule and is unused for another reason.
