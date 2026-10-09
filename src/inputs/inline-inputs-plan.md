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

- [x] Audit completed against baseline `2b6569c`; source paths already worked.
- [x] Parsing, inheritance/conflicts, diagnostics and CLI: `c71ef14`.
- [x] Nine integration tests for both workflows and JSON/editor diagnostics: `c71ef14`.
- [x] Binary-verified command demo and permanent guides: `c71ef14`.
- [x] Editor grammar, docs and tests: spit-vscode `79a818a`.
- [x] All required checks, strict docs and Messie passed for `c71ef14`.
- [x] Performance and seven byte-for-byte output comparisons passed for `c71ef14`.
- [x] Linked compiler PR #127 and editor PR #32 (stacked on editor #30).
- [ ] Delete this plan in the final commit, with recovery instructions.

No implementation steps are deferred. Merge the compiler before the editor;
merge editor #30 before retargeting #32 to main.
