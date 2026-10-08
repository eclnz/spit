# Check binding lookups

- Measure `spit check` on pipelines with increasing step, check, and stage counts before changing binding.
- Index check declarations and stage defaults once for binding, while preserving first-declaration and output ordering semantics.
- Add a scale regression test if the measured cost is material.
- Run required release, test, clippy, answer-key and benchmark checks. Record comparisons in the implementation commit.
- Delete this plan in a final commit before merging, preserving it in Git history.
