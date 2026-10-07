# Imports

`use` imports declarations from another pipeline file. It does not import that file’s top-level execution steps. Import paths are resolved relative to the importing file.

## Reuse definitions

Import operations, source families and [checks](checks.md#checks) from another `.spit` file. The path is relative to the file containing the `use` line. An operation brings its `command`; a source brings its path rule; either brings the checks it attaches. A source with companions declared [`beside`](paths.md#sidecar-files) it brings those companions with it. A companion cannot be imported alone: import its main source. A recipe names an imported main source as `text::raw_photo` when the import uses `as text`. An [operation carried out by steps](operations.md#operations-carried-out-by-steps) brings the operations its steps call. Imports do not bring pipeline steps.

```text
use text.spit as text

sorted = text::sort_lines(text::shard)
```

`as text` gives every imported name a prefix. Without it, `use text.spit` brings the names into the current scope. To import only a few definitions, use `use shard, sort_lines from text.spit as text`. A source imported as `text::shard` also uses that name in a recipe and a `.spitout`. SPIT reports missing names, import cycles, and name collisions. A message names a library by its path from the pipeline's folder, such as `libs/text.spit`, wherever the checkout is. An error in a library's own text is reported in the library, at its own line, and the `use` line that reads it follows as a related place:

```text
error: libs/text.spit: line 4, column 15: the body of `wrap` reads `nope`, which is neither one of its inputs nor made by an earlier step of it
  --> pipeline.spit: line 1, column 1: imported here
```

A library that imports a broken library lists each `use` line, nearest the error first. `spit check --json` gives the same: the diagnostic's `file` is `libs/text.spit`, and its `related` list holds each `use` line with its `file`.
