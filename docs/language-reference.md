# Language reference

This is the full syntax reference for `.spit` pipeline files. See the [README](../README.md) for a quick start and the [architecture](architecture.md) doc for the internal model.

A `#` that starts a word begins a comment, as in Bash. A `#` inside a word or in quotes is kept, so `--color=#fff` and `'#run'` are ordinary arguments.

## Products and dimensions

```text
source image : Image [subject, visit, run]
source reference [subject, visit]
```

Each `source` declares a product family, not an individual file. `image[subject=A,visit=1,run=2]` identifies one artifact. Types such as `Image` are optional; product names and entity bindings identify artifacts.

An assignment introduces a derived product automatically:

```text
processed = process(image)
average = mean(processed @ vary(run))
```

The input with the most dimensions drives a normal operation and gives its outputs their dimensions, wherever it sits among the ports. For an aggregation, `vary(run)` removes `run` from the output identity. You can write the output type and dimensions explicitly when helpful:

```text
average : Image [subject, visit] = mean(processed @ vary(run))
```

## Operations and commands

```text
operation process(image: Image) -> Image
command process: process_tool --in {image} --out {output}

operation mean(images: many Image) -> Image @ drop(run)
command mean: mean_tool {images} --out {output}
```

Declare an operation before its first use. Inputs in a call follow the port order in the declaration. A `one` input must resolve to exactly one artifact for each job, so every other input may only use dimensions the driving input has; SPIT rejects a pipeline that breaks this before reading any inventory, and reports a missing match for a job. A `many` input needs `@ vary(dimension)`, and its command placeholder expands to one separately quoted argument per artifact, ordered by the product's dimensions with numbers compared as numbers, so `run=2` comes before `run=10`. A many placeholder must occupy a whole argument. An operation takes at most one `many` input, which may sit beside `one` inputs; each of those is matched once per group:

```text
operation summarise(days: many Series, policy: Policy) -> Summary @ drop(day) @ min(2)
summary = summarise(reading @ vary(day), policy)
```

`@ min(2)` rejects a group with fewer than two artifacts.

Selectors narrow what an input matches:

```text
calibrated = calibrate(reading, calibration @ where(revision=2))
anomaly = compare(calibrated, reference @ same(station))
```

`where(revision=2)` keeps the artifacts with that value and takes `revision` out of matching, so a family with an extra dimension can join a less specific input. `same(station)` matches on `station` alone; the reference's other dimensions must then leave exactly one artifact for each job. Selectors can be combined, as in `frame @ where(acq=fast) @ vary(run)`.

An operation can write several outputs in one job. Name each output; its name is its placeholder, and the call assigns one product to each:

```text
operation estimate(dwi: DWI) -> (wm: Response, gm: Response, csf: Response)
command estimate: dwi2response dhollander {dwi} {wm} {gm} {csf}
wm_response, gm_response, csf_response = estimate(dwi)
```

A `verify` command checks a job's inputs before its command runs, using the tools that understand the files; if it fails, the script stops:

```text
verify register: check_same_grid {moving} {reference}
```

Input port names are optional. An unnamed single input is `{input}`; multiple unnamed inputs are `{input1}`, `{input2}`, and so on. Named ports give clearer errors, although errors also name the product bound to a port. `{output}` is the path of a single unnamed output, so `output` cannot name an input port. A command must use every output placeholder; a `verify` command may use inputs only. Command templates give ordered words and arguments, not shell pipelines or redirection. Words are split and quoted as in Bash, and every argument is passed literally: `$` and backticks are not expanded. Write `{{` or `}}` for a literal brace. Every command is checked when the pipeline is loaded: braces and quotes must balance, placeholders must name the operation's ports, and `{output}` must appear.

The first word of a command must be an executable available on `PATH` (or an executable path). SPIT emits that command without managing its installation or loading shell functions:

```text
command process: process_tool {image} {output}
```

## Reuse definitions

Import operations and source families from another `.spit` file. The path is relative to the file containing the `use` line. An operation brings its `command`; a source brings its path and coverage rules. Imports do not bring pipeline steps or inventory records.

```text
use text.spit as text

sorted = text::sort_lines(text::shard)
```

`as text` gives every imported name a prefix. Without it, `use text.spit` brings the names into the current scope. To import only a few definitions, use `use shard, sort_lines from text.spit as text`. A source imported as `text::shard` also uses that name in `sources:` or a separate inventory. SPIT reports missing names, import cycles, and name collisions.

## Paths

```text
path: results/{product}/{entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

`path:` sets a default. `path image:` overrides it for `image`. Each output of a multi-output step has its own product, so its own rule. Templates can use `{product}`, `{entities}`, or a declared dimension. In `{product}`, an imported `alias::name` becomes `alias.name`. Paths are relative to `SPIT_ROOT`.

Path rules are checked when the pipeline is loaded, even for products with no resolved jobs. SPIT rejects unbalanced braces, a dimension the product does not declare, a rule that omits one of the product's dimensions (use `{entities}` or name each one), and two products whose rules give the same path for the same entities, such as a default rule without `{product}`. Missing rules are reported by `--paths`, `bash`, and `--root`, and collisions between resolved artifact paths once jobs are bound.

As in Bash, an unquoted `#` starts a comment only at the start of a word, so `--color=#fff` is one argument. A `#` that ends a word, as in `{output}# note`, stays part of the word; SPIT warns about it, since it reads like a comment. Put a space before `#` to start a comment, or quote the text to keep it.

Place a source path beside its `source` line and a derived path beside its assignment. The default can stay near the top of the file.

Path rules also find sources. `spit discover pipeline.spit --root data` lists each file under `data` whose path matches a source's rule, reading entity values from its placeholders, as inventory text. Other commands given `--root` and no inventory do the same, so `spit bash pipeline.spit --root data` needs no inventory file.

## Constraints and optional types

```text
require image count>=2 per [subject, visit]
require reference count=1 per [subject, visit]
```

Constraints check each observed group. They do not set a total subject or visit count. A rule can also require particular values in each group, alone or with a count:

```text
require image run=1,2 per [subject, visit]
```
 An inventory may include `contexts:` to name a group even when one of its required inputs is absent:

```text
contexts:
    [subject=A,visit=1]
sources:
    image[subject=A,visit=1,run=1]
```

Types are additive. You can leave them out, add them to selected products and operations, or type the whole pipeline. Known mismatches fail; missing type information does not.

In operation signatures, a single capital letter such as `S` is a local type variable. Use a `$` prefix for longer names, such as `$SourceSpace` or `$Kind`. An unprefixed name such as `World` is a concrete type. Variables are allowed in operation signatures, not product declarations:

```text
operation project(sample: Frame<$Kind,$SourceSpace>, calibration: Calibration<$Kind,$SourceSpace,$TargetSpace>) -> Frame<$Kind,$TargetSpace>
```

## Grouped sections

SPIT also accepts grouped `products:`, `operations:`, `pipeline:`, and `constraints:` sections, as an alternative to the flow style used elsewhere in this reference. The flow style is intended for writing a pipeline in the order you read it.
