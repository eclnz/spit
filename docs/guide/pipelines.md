# Pipelines

A `.spit` pipeline describes product families and steps without listing a dataset. See the [pipeline language manual](../language-reference.md#products-and-dimensions) for exact declaration rules.

A `.spit` pipeline declares sources, operations, commands, and steps. Declarations describe reusable rules; a step applies an operation to product families. Put sources and `use` imports at the top level. Operations may be declared globally or in a stage, but their names are global.

## Operations

<!-- spit-doc: setup
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
source image : Image .nii.gz [sub]

operation denoise(input: Image) -> Image .nii.gz
command denoise: denoise_tool {input} --out {@output}
denoised = denoise(image)
```

An input port is `name` or `name: Type`; add `many` for a collection. An output can be unnamed, as above, or named. Calls follow input port order. A named output is assigned to its corresponding product and used in a command by its port name:

<!-- spit-doc: setup
source series : Series [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation split(series: Series) -> (low: Series .csv, high: Series .csv)
command split: split_tool {series} --low {low} --high {high}
low_band, high_band = split(series)
```

Every non-`beside` output must be used in its command. The command is an executable with literal arguments, not a shell script; SPIT expands placeholders into paths, and a dimension's placeholder into the job's value, when it builds the DAG. A `many` input expands to one argument per artifact. A job that runs two tools in turn, as one that sorts a file and then indexes it, gives each a line of its own beneath `command name:` (see [commands of several lines](../language-reference.md#commands-of-several-lines)). Quoting rules, generic types, multi-output syntax and shell metacharacter restrictions are in [operations and commands](../language-reference.md#operations-and-commands).

## Checks

Use `verify` for a job-specific input test, before its main command:

<!-- spit-doc: setup
source image : Image [sub]
operation denoise(input: Image) -> Image
command denoise: denoise_tool {input} --out {@output}
denoised = denoise(image)
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
verify denoise: inspect_image {input}
```

Use a `check` to test one artifact, whether it is a source, an input port, or an output. An output check runs after the command:

```spit
check nonempty: test -s {@path}
operation denoise(input: Image) -> Image .nii.gz @check(nonempty)
```

SPIT records these commands in the DAG; the runner performs them. A failed check or verify fails the job. Checks can take declared parameters, such as `check ndim(n): check_ndim {@path} {n}` and `@check(ndim(3))`. See [checks](../language-reference.md#checks) for attachment points and execution order.

For checks shared by every output in a file or stage, write `default check: nonempty, ndim(3)` once. A nested stage can remove an inherited check with `default check: !nonempty`; an output can do the same with `@check(!nonempty)`. See [default checks](../language-reference.md#default-checks).

## Cores and memory

A runner starts several jobs at once. Say what each job of an operation needs, so that it starts only as many as the machine can hold, and pass the cores to the tool with `{@cores}`:

<!-- spit-doc: setup
source image : Image [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation denoise(input: Image) -> Image .nii.gz
command denoise: denoise_tool --threads {@cores} {input} --out {@output}
resources denoise: cores=4, memory=8G
denoised = denoise(image)
```

Each job runs `denoise_tool --threads 4 ...`. An operation with no `resources` line needs one core and claims no memory. See [cores and memory](../language-reference.md#cores-and-memory).

## A job's values

A placeholder that names one of the job's dimensions gives the tool that job's value, as text:

<!-- spit-doc: setup
source image : Image [sub, ses]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation plot(input: Image) -> Figure .png
command plot: plot_tool {input} --title "subject {sub}, session {ses}" --out {@output}
figure = plot(image)
```

The job for `image[sub=01,ses=2]` runs `plot_tool ... --title 'subject 01, session 2' ...`. A job has the dimensions of its outputs, so a step that collects sessions with `@vary(ses)` has no one `{ses}`, and `spit check` says so. Such a step's `{items.ses}` gives each collected artifact's session instead, one argument per artifact, in the order `{items}` gives their paths. See [a job's dimension values](../language-reference.md#a-jobs-dimension-values).

## Composite operations

An operation can have an indented body in place of a `command`. Its outputs are named because the body assigns each one:

<!-- spit-doc: setup
source raw : Lines [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

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

<!-- spit-doc: setup
source image : Image [sub]
operation denoise(input: Image) -> Image
command denoise: denoise_tool {input} --out {@output}
operation summarise(items: many Image) -> Image
command summarise: summarise_tool {items} --out {@output}
-->

```spit
default entities: {@key}={@value} separated "__"
stage preprocess:
    default path: derived/{@stage}/{@product}/{@entities}
    denoised = denoise(image)

stage analysis:
    default path: results/{@product}
    summary = summarise(denoised @vary(sub))
```

A stage groups steps and can give their products a path and extension default. Stages can nest; a product's `{@stage}` is its stage path. Stages do not impose execution order: product dependencies do. Stage syntax and rules are in [stages](../language-reference.md#stages).

## Imports

<!-- spit-doc: setup
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
--- text.spit = examples/imports/text.spit
-->

```spit
use text.spit as text
sorted = text::sort_lines(text::shard)
```

`use` imports definitions from a file relative to the importing file. An operation brings its command or body and attached checks; a source brings its path rule and attached checks. Top-level steps are not imported. See [reuse definitions](../language-reference.md#reuse-definitions) for selection, prefixes and conflicts.

Next: [Types](types-and-reuse.md).
