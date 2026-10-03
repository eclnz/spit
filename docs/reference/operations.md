# Operations and calls

SPIT defines the syntax of an operation; **the pipeline author defines every operation name**. `clean`, `align`, and `train` have no built-in meaning. Their declared ports, command, and calls determine what jobs SPIT plans. Use this page to look up the forms; [operations and commands](../language-reference.md#operations-and-commands) covers the full matching and validation rules.

## Declare an operation

```spit
operation clean(input: Image) -> Image .nii.gz
operation merge(items: many Lines @ min(2)) -> Lines
operation split(series: Series) -> (low: Series .csv, high: Series .csv)
```

An operation must be declared before its first call. The name after `operation` is global, including when the declaration is inside a stage. Inputs go in parentheses and calls fill them in that order. A single output can be unnamed after `->`; several outputs are enclosed in parentheses and each has a port name. Types and output extensions may be omitted. `operation copy(input)` is an untyped single-output form.

## Input and output forms

| Form | Example | Effect |
| --- | --- | --- |
| Untyped input | `input` | Accepts one artifact without a type check. |
| Typed input | `input: Image` | Accepts one artifact; a known type mismatch is an error. |
| Collection input | `items: many Lines` | Accepts several artifacts; its call uses `@ vary(...)`. An operation has at most one `many` input. |
| Minimum collection | `items: many Lines @ min(2)` | Requires at least two complete members in each collection. |
| Single output | `-> Image .nii.gz` | One unnamed output, available in the command as `{@output}`. |
| Named outputs | `-> (low: Series .csv, high: Series .csv)` | One job writes both; the command uses `{low}` and `{high}`. |
| Folder output | `-> FsSubject /` or `-> Zarr .zarr/` | The artifact is a folder, not a file. |
| Companion output | `meta: Json .json beside image` | The tool writes `meta` beside `image`; no separate path argument or rule is needed. |

In a typed signature, one uppercase letter (`S`) or a `$`-prefixed name (`$Space`) is a local type variable. SPIT binds it separately for each call. See [optional types](../language-reference.md#optional-types), [folders](../language-reference.md#folders), and [files written beside another](../language-reference.md#files-a-tool-writes-beside-another).

## Call an operation

```spit
cleaned = clean(image)
low_band, high_band = split(series)
daily = merge(sorted @ vary(part))
```

A step assigns a product name to each output. Its inputs are product families, optionally with [selectors](selectors.md). `@ vary(part)` collects all matching parts into a `many` port and removes `part` from the output identity. A normal input must resolve to exactly one artifact for a job. An output can annotate the type and dimensions it expects, such as `daily : Lines [group] = merge(sorted @ vary(part))`; SPIT checks that annotation against inference. The step, rather than the operation declaration, determines the product names used by later steps.

## Command templates

```spit
command clean: clean_tool {input} --out {@output}
command split: split_tool {series} --low {low} --high {high}
```

`{input}` is an input port's path, `{low}` a named output's path, and `{@output}` the single unnamed output's path. A `many` input placeholder expands to one argument per artifact in dimension order and must occupy an entire argument. Every output must be used by the command, except an output declared `beside` another. For tools that accept a destination folder and name instead of a full path, use `{output.dir}` and `{output.stem}`; those count as using the output.

The first word is an executable on `PATH` or an executable path. SPIT records arguments, not shell syntax: pipes, redirection, and shell expansion are not performed. Quoting groups literal arguments; paths are filled in as arguments rather than evaluated by a shell. See [placeholders](placeholders.md#command-placeholders) and [command rules](../language-reference.md#operations-and-commands).

## Verification and artifact checks

```spit
verify align: inspect_grid {moving} {reference}
check nonempty: test -s {@path}
operation align(moving: Image, reference: Image) -> Image @ check(nonempty)
```

`verify` uses input ports and runs before that job's command. A `check` tests one artifact: attach it to a source, input port, or output with `@ check(...)`; an output check runs after the command. SPIT writes both into the `.spitdag`, and a runner executes them. See [checks](../language-reference.md#checks) and the [DAG format](../spitdag.md#checks).
