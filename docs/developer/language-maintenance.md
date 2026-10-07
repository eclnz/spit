# Language maintenance

The manual owns accepted syntax and observable semantics. The guide owns teaching
sequences and links to those rules. Keep implementation details in developer pages.
When behavior changes, update its canonical specification and examples together.

## Coverage matrix

This initial topic map identifies the specification and existing evidence. It is
not yet a completed rule-by-rule audit; the remote continuation must expand it and
verify the rewritten examples against the binary.

| Topic | Canonical specification | Implementation and test evidence |
| --- | --- | --- |
| Names, comments, indentation | [Foundations](../manual/foundations.md) | `src/parser/lexical.rs`, `src/parser/flow.rs`, `tests/parser.rs`, `tests/syntax_errors.rs` |
| Products and dimension order | [Pipeline](../manual/pipeline.md) | `src/order.rs`, `tests/order.rs`, `tests/resolver.rs` |
| Operations and body expansion | [Operations](../manual/operations.md) | `src/parser/operation.rs`, `src/lower/expand.rs`, `tests/composites.rs`, `tests/multiple_outputs.rs` |
| Matching and selectors | [Matching](../manual/matching.md) | `src/resolver`, `tests/matching.rs`, `tests/coverage.rs` |
| Type compatibility | [Types](../manual/types.md) | `src/types.rs`, `tests/types.rs`, `tests/type_errors.rs` |
| Stages and imports | [Pipeline](../manual/pipeline.md#stages), [Imports](../manual/reuse.md) | `tests/stages.rs`, `tests/imports.rs`, `tests/imported_errors.rs` |
| Paths, extensions, shapes, folders, companions | [Paths](../manual/paths.md) | `src/paths`, `tests/paths.rs`, `tests/extensions.rs`, `tests/placeholder_shapes.rs`, `tests/folders.rs`, `tests/beside.rs` |
| Checks | [Checks](../manual/checks.md) | `src/check.rs`, `tests/checks.rs` |
| Recipes and rule ordering | [Recipe](../manual/recipe.md) | `src/inputs`, `tests/discovery.rs`, `tests/exclusions.rs`, `tests/conditional_exclusions.rs`, `tests/two_files.rs` |
| Inventory and roots | [Input inventory](../manual/inventory.md), [CLI roots](../manual/cli.md#root-selection) | `src/parser/inventory.rs`, `src/parser/render_inventory.rs`, `tests/inputs.rs`, `tests/dataset_root.rs` |
| DAG serialization and runner obligations | [Runnable DAG](../manual/dag.md) | `src/spitdag`, `tests/outputs.rs`, `tests/folder_and_stem.rs` |
| CLI and editor diagnostics | [Command line](../manual/cli.md) | `src/cli`, `tests/cli.rs`, `tests/severity.rs`, `tests/hovers.rs`, `tests/builtin_words.rs` |

Two contradictions in the former reference have been resolved from tests:
`--root` cannot override recipe or inventory roots (`tests/dataset_root.rs`), and a
stage can be reopened at the same nesting level (`tests/stages.rs`). Verify these
with the binary before considering the rewrite final.
