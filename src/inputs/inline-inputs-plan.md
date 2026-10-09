# Basic inputs in a pipeline (#107)

Allow a small pipeline to locate its data and run as one `.spit` file.
Source paths and their placeholder shapes already work in a pipeline. Add
one top-level `root`, relative to that pipeline's folder. Keep selection
policy in `.spitin`: `discover`, `require`, every `exclude` form (including
literal exclusions and external lists), and written inventory records.
This boundary depends on the declaration's meaning, not its size.

A recipe may inherit the pipeline's root. Roots in both files are errors,
even if equal; an explicit source path in both files remains an error.
Defaults retain their existing semantics: a recipe default fills sources
without a pipeline rule. A command-line root is relative to the working
folder and is accepted only when the pipeline has no root. A saved
inventory keeps its own root and remains independent of inline roots.
Imports bring definitions, never another file's root.

- [x] Audited baseline `2b6569c`: source paths accepted; root and selection rules rejected.
- [x] Implementation complete; record its commit after the checks pass.
- [x] Nine new integration tests cover both workflows, paths, imports and JSON errors.
- [x] Command demo verified: three source files and five jobs, with either file. Docs updated.
- [x] Matching editor branch updated; 52 tests pass, one pre-existing skip. PR links pending.
- [ ] Run all required checks and performance/output comparisons.
- [ ] Record implementation commits, then delete this plan alone before merge.
