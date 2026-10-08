# Language catalog

Use these pages to look up a particular piece of syntax without following a walkthrough. The [complete language reference](../language-reference.md) gives the full rules and interactions.

| Browse | What it covers |
| --- | --- |
| [Keywords and records](keywords.md) | Every statement keyword in `.spit` and `.spitin`, plus `.spitout` section headers |
| [Operations and calls](operations.md) | Operation signatures, ports, outputs, commands, checks, and step assignments |
| [Selectors and clauses](selectors.md) | `where`, `same`, `vary`, `each`, `min`, `check`, `beside`, and recipe conditions |
| [Placeholders](placeholders.md) | Path, command, and check placeholders, plus built-in source shapes |

**Operation names are user-defined.** SPIT has no built-in `sort`, `align`, or `train` operation to enumerate. An `operation` declaration defines its ports and outputs; a `command` defines the program it will run; a step calls it. The [operations page](operations.md) lists every form those declarations and calls can take.

## Choose by file

- Writing a pipeline? Start with [pipeline keywords](keywords.md#pipeline-keywords), then [operations and calls](operations.md).
- Writing a dataset recipe? See [recipe keywords](keywords.md#recipe-keywords) and [recipe clauses](selectors.md#recipe-conditions).
- Reading an input inventory? See [inventory headers](keywords.md#inventory-headers).
- Filling a path or command? See [placeholders](placeholders.md).
