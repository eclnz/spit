# Connected pipeline diagrams (#103)

Complete in the accompanying implementation commit:
- Connected product lines with bracketed products, parenthesised operations and downward arrows.
- Explicit crossings; one line per shared product, indexed dependencies, iterative traversal and lowest-free-column reuse.
- Separate, deduplicated diagrams for reusable and nested operation bodies.
- Exact join layout, 10,000-step chain, nested components and both MRtrix3 examples tested.
- Release build, full tests, clippy, answer keys and pipeline/dataset regression checks passed.
- Scaling checked at 1000/2000/4000 ordinary and modular steps; output grows linearly and chains remain narrow.
- Publish the update to PR #118. Local Messie remains blocked by its unavailable tokenizer cache; CI must verify it.
