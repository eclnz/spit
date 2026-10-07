# User manual

This manual defines the accepted language and observable behavior of SPIT. It stands alone: syntax, scope, defaults, constraints, interactions, and failure conditions are specified here. Examples illustrate rules; they do not override them. Current behavior takes precedence over proposed features in the issue tracker.

Read the [foundations](foundations.md) for terminology and grammar notation. The four file specifications are [Pipeline](pipeline.md), [Recipe](recipe.md), [Input inventory](inventory.md), and [Runnable DAG](dag.md). The [command line](cli.md) defines how files enter and leave SPIT.

## Syntax index

| Construct | Canonical rules |
| --- | --- |
| Names, comments, indentation, grammar | [Foundations](foundations.md) |
| `source`, assignments, `dimensions` | [Products](pipeline.md#products-and-dimensions), [dimension order](pipeline.md#dimension-order) |
| `operation`, ports, outputs, `command`, `verify` | [Operations](operations.md#operations-and-commands) |
| Operation bodies | [Operations carried out by steps](operations.md#operations-carried-out-by-steps) |
| `where`, `same`, `vary`, `each`, `many`, `min` | [Matching](matching.md) |
| Types and type variables | [Types](types.md) |
| `stage` | [Stages](pipeline.md#stages) |
| `use`, `from`, `as` | [Imports](reuse.md#reuse-definitions) |
| `path`, placeholders, optional groups | [Paths](paths.md#paths) |
| `ext:`, extensions, shapes, folders, `beside` | [Extensions](paths.md#extensions), [shapes](paths.md#shapes-on-a-source-placeholder), [folders](paths.md#folders), [output companions](paths.md#files-a-tool-writes-beside-another), [source companions](paths.md#sidecar-files) |
| `check`, `@ check`, `check:`, `!` | [Checks](checks.md#checks), [default checks](checks.md#default-checks) |
| `pipeline`, `root`, source rule precedence | [Recipe](recipe.md#recipes), [file ownership](recipe.md#which-file-a-line-belongs-in) |
| `discover`, `from dirs` | [Discovery](recipe.md#discover-contexts-from-directories) |
| `require`, `count`, `has`, `missing` | [Constraints](recipe.md#constraints) |
| `exclude`, conditions, CSV exclusions | [Conditional exclusions](recipe.md#exclude-groups-that-meet-a-condition), [named exclusions](recipe.md#exclude-named-artifacts) |
| `sources:`, `contexts:`, `source_paths:`, `removed:` | [Inventory](inventory.md#inputs) |
| JSON fields, commands, runner obligations | [Runnable DAG](dag.md) |
| CLI forms, flags, roots, diagnostics | [Command line](cli.md) |
