# Types and reusable definitions

Types help SPIT catch wiring mistakes while the pipeline is compiled. They are optional: an untyped product is allowed, and missing type information does not itself fail a step. Add types to the boundaries where a mistaken connection would be costly.

## Declare and check types

```spit
source raw : Frame<Native> [sample]
source reference : Reference<Target> [sample]

operation clean(frame: Frame<S>) -> CleanFrame<S>
cleaned = clean(raw)

operation align(frame: CleanFrame<A>, reference: Reference<B>) -> Transform<A,B>
transform = align(cleaned, reference)
```

The uppercase `S`, `A`, and `B` in operation signatures are local type variables. Each call binds them from its inputs, then uses those bindings to infer its output type. `Frame<Native>` and `Reference<Target>` are concrete product types. A known mismatch is an error; an unknown type can still pass through a call. The complete rules and longer `$Variable` spelling are in [optional types](../language-reference.md#optional-types). The checked-in [typed example](https://github.com/eclnz/spit/blob/dev/examples/types/typed.spit) uses this exact pipeline.

Types do not select artifacts. Product names and dimensions determine which source artifacts exist and which one a job reads. See [matching and collections](matching.md) for that part of resolution.

## Import reusable declarations

```spit
use text.spit as text
sorted = text::sort_lines(text::shard)
```

`use` reads a `.spit` file relative to the importing file. With `as text`, imported names are qualified as `text::...`; without `as`, they enter the current scope. To select only some definitions, write `use shard, sort_lines from text.spit as text`.

An imported operation carries its `command` and checks attached to its ports. An imported source carries its path rule and attached checks. A check can also be imported by name. Imports do not bring in the other file's steps, so a library file can define reusable pieces without adding jobs to the caller. Cycles, unknown names, and collisions are errors. See [reuse definitions](../language-reference.md#reuse-definitions) for the precise selection rules and the [import example](https://github.com/eclnz/spit/blob/dev/examples/imports/imported.spit) for a small complete file.

## Keep execution checks separate from types

A type describes a logical file kind, such as `Frame<Native>`. SPIT does not read file contents to prove that a file is a valid frame. Use a `check` on one artifact or `verify` on a job's inputs for those runtime tests. [Pipelines and operations](pipelines.md) explains both.
