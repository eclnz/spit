# Reuse

Put shared operations and sources in a library pipeline when several pipelines need the same definitions. Import the declarations, then write each pipeline’s own calls.

## Import reusable declarations

```spit
use text.spit as text
sorted = text::sort_lines(text::shard)
```

`use` reads a `.spit` file relative to the importing file. With `as text`, imported names are qualified as `text::...`; without `as`, they enter the current scope. To select only some definitions, write `use shard, sort_lines from text.spit as text`.

An imported operation carries its `command` and checks attached to its ports. An operation carried out by steps also brings the operations and checks its body needs. An imported source carries its path rule and attached checks. A check can also be imported by name. Imports do not bring in the other file's top-level steps, so a library file can define reusable pieces without adding jobs to the caller. Cycles, unknown names, and collisions are errors. See [reuse definitions](../manual/reuse.md#reuse-definitions) for the precise selection rules and the [import example](https://github.com/eclnz/spit/blob/dev/examples/imports/imported.spit) for a small complete file.

Next: [Recipe](recipes.md).
