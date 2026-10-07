# Types

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

The uppercase `S`, `A`, and `B` in operation signatures are local type variables. Each call binds them from its inputs, then uses those bindings to infer its output type. `Frame<Native>` and `Reference<Target>` are concrete product types. A known mismatch is an error; an unknown type can still pass through a call. The complete rules and longer `$Variable` spelling are in [optional types](../manual/types.md#optional-types). The checked-in [typed example](https://github.com/eclnz/spit/blob/dev/examples/types/typed.spit) uses this exact pipeline.

Types do not select artifacts. Product names and dimensions determine which source artifacts exist and which one a job reads. See [matching and collections](matching.md) for that part of resolution.

## Keep execution checks separate from types

A type describes a logical file kind, such as `Frame<Native>`. SPIT does not read file contents to prove that a file is a valid frame. Use a `check` on one artifact or `verify` on a job's inputs for those runtime tests. [Pipelines and operations](pipelines.md) explains both.

Next: [Reuse](reuse.md).
