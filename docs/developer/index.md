# Developer documentation

The [user manual](../manual/index.md) defines observable language behavior. These pages explain how the compiler implements it. Keep protocol rules in the manual rather than duplicating them here.

| Subject | Implementation concern |
| --- | --- |
| [Compilation](compilation.md) | Parsing, imports, lowering, types, and stage boundaries |
| [Input settlement](inputs.md) | Discovery, matching paths, rules, and inventory rendering |
| [Resolution](resolution.md) | Job candidates, completeness, path binding, and command expansion |
| [Data layout](data-layout.md) | Ownership, ids, shared tables, and step-level data |
| [Diagnostics](diagnostics.md) | Severity, locations, recovery, and editor protocol |
| [Performance](performance.md) | The five performance rules and regression checks |
| [Language maintenance](language-maintenance.md) | Canonical specification coverage and evidence |

Compilation, input settlement, and resolution remain separate stages. Each passes plain data to the next; the CLI and diagnostics coordinate them. A runner executes the [DAG contract](../manual/dag.md), using no pipeline or recipe.
