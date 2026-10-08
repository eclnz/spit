# Pipelines

A `.spit` pipeline describes product families and steps without listing a dataset. See the [pipeline language manual](../language-reference.md#products-and-dimensions) for exact declaration rules.

A `.spit` pipeline declares sources, operations, commands, and steps. Declarations describe reusable rules; a step applies an operation to product families. Put sources and `use` imports at the top level. Operations may be declared globally or in a stage, but their names are global.

## Operations

```spit
source image : Image .nii.gz [sub]

operation denoise(input: Image) -> Image .nii.gz
command denoise: denoise_tool {input} --out {@output}
denoised = denoise(image)
```

An input port is `name` or `name: Type`; add `many` for a collection. An output can be unnamed, as above, or named. Calls follow input port order. A named output is assigned to its corresponding product and used in a command by its port name:

```spit
operation split(series: Series) -> (low: Series .csv, high: Series .csv)
command split: split_tool {series} --low {low} --high {high}
low_band, high_band = split(series)
```

Every non-`beside` output must be used in its command. The command is an executable with literal arguments, not a shell script; SPIT expands placeholders into paths when it builds the DAG. A `many` input expands to one argument per artifact. Quoting rules, generic types, multi-output syntax and shell metacharacter restrictions are in [operations and commands](../language-reference.md#operations-and-commands).

## Checks

Use `verify` for a job-specific input test, before its main command:

```spit
verify denoise: inspect_image {input}
```

Use a `check` to test one artifact, whether it is a source, an input port, or an output. An output check runs after the command:

```spit
check nonempty: test -s {@path}
operation denoise(input: Image) -> Image .nii.gz @ check(nonempty)
```

SPIT records these commands in the DAG; the runner performs them. A failed check or verify fails the job. Checks can take declared parameters, such as `check ndim(n): check_ndim {@path} {n}` and `@ check(ndim(3))`. See [checks](../language-reference.md#checks) for attachment points and execution order.

For checks shared by every output in a file or stage, write `check: nonempty, ndim(3)` once. A nested stage can remove an inherited check with `check: !nonempty`; an output can do the same with `@ check(!nonempty)`. See [default checks](../language-reference.md#default-checks).

## Composite operations

An operation can have an indented body in place of a `command`. Its outputs are named because the body assigns each one:

```spit
operation clean(input: Lines) -> Lines
command clean: clean_tool {input} {@output}

operation twice(input: Lines) -> (result: Lines):
    first = clean(input)
    result = clean(first)

cleaned = twice(raw)
```

The call expands to the body's steps, and a body may call another operation with a body. The body can be imported as one operation with the operations its steps need. `spit check pipeline.spit --calls` lists the expansion before reading a dataset. See [operations carried out by steps](../language-reference.md#operations-carried-out-by-steps).

## Stages

```spit
stage preprocess:
    path: derived/{@stage}/{@product}/{@entities}
    denoised = denoise(image)

stage analysis:
    summary = summarise(denoised @ vary(sub))
```

A stage groups steps and can give their products a path and extension default. Stages can nest; a product's `{@stage}` is its stage path. Stages do not impose execution order: product dependencies do. Stage syntax and rules are in [stages](../language-reference.md#stages).

## Imports

```spit
use text.spit as text
sorted = text::sort_lines(text::shard)
```

`use` imports definitions from a file relative to the importing file. An operation brings its command or body and attached checks; a source brings its path rule and attached checks. Top-level steps are not imported. See [reuse definitions](../language-reference.md#reuse-definitions) for selection, prefixes and conflicts.

Next: [Types](types-and-reuse.md).
