# Selectors and clauses

These forms appear inside an operation call, an operation signature, or a recipe rule. They are not standalone pipeline statements. See [matching and collections](../guide/matching.md) for how artifact identities become jobs.

## Call selectors

| Selector | Example | What changes |
| --- | --- | --- |
| `@ where(...)` | `calibration @ where(revision=2)` | Keep that value, then remove the selected dimension from matching. |
| `@ same(...)` | `reference @ same(station)` | Match only on the named dimensions; every job still needs exactly one matching artifact. |
| `@ vary(...)` | `processed @ vary(run)` | Feed a `many` port a collection over the named dimensions; those dimensions leave the output identity. |
| `@ each(...)` | `model @ each(scenario)` | Broadcast over observed values of a dimension the driving input lacks; outputs gain that dimension. |

Selectors belong to a *call's input*, after its product name. They can be combined, for example `frame @ where(acq=fast) @ vary(run)`. One `@ vary` can name several dimensions: `@ vary(model, config)`. Its written order does not change collection order; the pipeline's dimension order does. A call may not write two `@ vary` clauses. See [operations and commands](../language-reference.md#operations-and-commands) for all restrictions.

## Signature and output clauses

| Clause | Example | Meaning |
| --- | --- | --- |
| `many` | `runs: many Image` | Input port accepts a collection; an operation has at most one. |
| `@ min(n)` | `runs: many Image @ min(2)` | Reject collections smaller than `n`, after incomplete members are removed by `dag --partial`. |
| `@ check(...)` | `-> Image @ check(nonempty)` | Attach a declared artifact check to a source, input port, or output. Multiple checks can be listed. |
| `beside` | `meta: Json .json beside image` | Output follows another output's path stem and must be written by the tool; a source companion can use the same clause. |
| `/` | `-> FsSubject /` | Output is a folder; a source can use `/` in the same place. |

`@ min` is declared on a `many` port, not on an operation's result. `@ check` runs through the backend, not through SPIT itself. See [operation forms](operations.md#input-and-output-forms), [checks](../language-reference.md#checks), and [sidecar output rules](../language-reference.md#files-a-tool-writes-beside-another).

A file or stage can set output checks with `check: nonempty, ndim(3)`. A nested stage or an output can remove an inherited default with `!nonempty`; the output's own attached check remains. See [default checks](../language-reference.md#default-checks).

## Recipe conditions

| Clause | Example | Meaning |
| --- | --- | --- |
| `count` | `require [sub, ses] where t1w count=1` | Compare a source's artifact count or a discovery's context count with `=`, `!=`, `>=`, `<=`, `>` or `<`. |
| `where` in a recipe | `exclude [sub] where sessions count<2` | Introduce a group's condition in `exclude` or `require`. This is distinct from a call's `@ where(...)`. |
| `missing` | `exclude [sub, ses] where bold missing run=1,2` | Remove a group if any listed value is absent. |
| `has` | `require [sub, ses] where bold has run=1,2` | Check that listed values are present; `exclude` may use `has` to remove groups instead. |
| `from dirs` | `discover sessions: [sub, ses] from dirs sub-{sub}/ses-{ses}` | Give `discover` its directory pattern. |
| `from` in `exclude` | `exclude from qc/excluded.csv` | Read exclusion rows from a CSV file relative to the recipe. |

Named exclusions apply first, then conditional exclusions, then `require`, regardless of line order. A requirement can check values, as `require [sub, ses] where image has run=1,2`. See [recipes](../language-reference.md#recipes) for groups, errors, and precedence.
