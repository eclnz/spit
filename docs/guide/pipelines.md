# Pipelines

A pipeline is a `.spit` file. It declares the families of artifacts that exist, the operations that make new ones, and the steps that connect them. It describes the work, not the dataset: the same pipeline plans a different set of jobs for each dataset it is given.

## Syntax

A pipeline is a list of statements, and a statement's first word says what it is. The words that start statements are reserved, so a product cannot be called `source`, `stage`, `path`, `default` and so on. Use descriptive names for products, since they appear in paths, plans and messages.

- An unquoted `#` at the start of a word begins a comment. A `#` inside a word, as in `--color=#fff`, is kept.
- A keyword alone with a `:` opens a block of its statements, each indented beneath it without repeating the keyword:

<!-- spit-doc: setup
operation denoise(image: Image) -> Image
-->

```spit
source:
    raw : Image [sub]
    mask : Mask [sub]

command:
    denoise: denoise_tool {image} {@output}
```

- A statement carries on to the next lines while its brackets are open, so a long signature or a step with many arguments can be laid out one item to a line.

## Steps

A step applies an operation to products and names what it makes:

<!-- spit-doc: setup
source shard : Lines [group, part]
operation sort_lines(input: Lines) -> Lines
operation merge(items: many Lines) -> Lines
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
sorted = sort_lines(shard)
merged = merge(sorted @vary(part))
```

The product on the left is new, and its type and dimensions come from the operation and its inputs. Each `merge` job depends on the `sort_lines` jobs that make the sorted files it reads, so the order of the jobs follows from the products and not from the order of the lines.

- Declare an operation before its first call.
- Steps can appear in any order, and a step can read a product that a later line makes.
- A step cannot read its way back to its own output. SPIT reports the loop once, naming it:

<!-- spit-doc: output
spit check cyc.spit || true
--- cyc.spit
source raw : Lines [id]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation join(a: Lines, b: Lines) -> Lines
command join: cat {a} {b} > {@output}
x = join(raw, y)
y = join(raw, x)
-->

```text
error: line 6, column 1: pipeline cycle: x -> y -> x
```

Break the loop by removing one dependency, or by adding a separate input.

## Matching

A job needs one artifact for each of its inputs. SPIT decides which, using dimensions:

- The input with the most dimensions **drives** the step. Each artifact of the driving input that exists is one job, and the job's output takes the driving input's dimensions.
- Every other input is matched at the driving artifact's dimension values.
- An input with no dimensions matches every job.

<!-- spit-doc: setup
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
source reading : Series [station, day]
source policy : Policy [station]
operation calibrate(series: Series, policy: Policy) -> Series
calibrated = calibrate(reading, policy)
```

Each `reading[station=...,day=...]` is a job, and the job reads the `policy` for the same station. SPIT never picks a file silently. A job whose input has no match, or more than one, is an error that names the input and the values:

<!-- spit-doc: output
spit dag amb.spit amb.spitout || true
--- amb.spit
source reading : Series [station, day]
source reference : Series [station, rev]
path reading: r/{station}/{day}.csv
path reference: f/{station}/{rev}.csv
operation compare(series: Series, reference: Series) -> Series
command compare: cmp {series} {reference} {@output}
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}.csv
anomaly = compare(reading, reference @same(station))
--- amb.spitout
sources:
    reading[station=a,day=1]
    reference[station=a,rev=1]
    reference[station=a,rev=2]
-->

```text
error: line 9, column 28: more than one `reference` artifact matches input `reference` of `compare` at [day=1,station=a]; select one with `@where(...)`
```

The first step in finding a missing or ambiguous match is to look at the candidates: [Plans](plans.md#missing-work) shows how. Then add a selector that leaves one:

- `reference @where(rev=2)` keeps the artifacts with that value and takes `rev` out of matching.
- `reference @same(station)` matches on `station` alone. If two references still match, the job is ambiguous, as above.

A selector can sit beside a collection, as in `frame @where(acq=fast) @vary(run)`, which filters to the fast acquisition and then gathers its runs.

## Collections

A `many` port takes a group of artifacts, and the step says which dimensions to gather:

<!-- spit-doc: setup
source shard : Lines [group, part]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation sort_lines(input: Lines) -> Lines
sorted = sort_lines(shard)
-->

```spit
operation merge(items: many Lines @min(2)) -> Lines
merged = merge(sorted @vary(part))
```

`@vary(part)` gathers every `part` within each `group`. The result has one artifact for each group and no `part` dimension. `@min(2)` rejects a group with fewer than two parts, which the beta group in the [first run](first-run.md) would be:

<!-- spit-doc: output
spit dag lm.spit lm.spitout || true
--- lm.spit
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt
default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)
operation merge(items: many Lines @min(2)) -> Lines
command merge: sort -m -u -o {@output} {items}
merged = merge(sorted @vary(part))
--- lm.spitout
sources:
    shard[group=alpha,part=1]
    shard[group=alpha,part=2]
    shard[group=beta,part=1]
-->

```text
error: line 10, column 16: input `items` of `merge` needs at least 2 artifacts at [group=beta], found 1
```

Several rules decide how a collection behaves:

- **Order.** A collection follows the pipeline's [dimension order](artifacts.md#dimension-order), and runs of digits compare as numbers, so part `2` comes before part `10`. The command's arguments are in the same order.
- **Several dimensions.** `@vary(model, config)` gathers over both in one clause. A call may have only one `@vary`.
- **Other inputs.** A `many` port can sit beside ordinary ports, which are matched once for each group.
- **Parallel collections.** An operation that takes two `many` ports, such as each region's heights and slopes, must be given the same `@vary` on both. The two collections must have the same dimensions and the same identities, and their placeholders list artifacts in the same order, so the two lists pair up by position. An artifact in one and not the other is an error that names it.
- **Selectors.** `@vary` combines with `@where`, as in the previous section.

## Broadcasting

`@each` is the reverse of `@vary`: it runs a step once for every value of a dimension that the driving input lacks, so the output gains that dimension:

<!-- spit-doc: setup
operation predict(series: Series, model: Model) -> Series
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
dimensions [station, scenario]
source reading : Series [station]
source model : Model [scenario]
forecast = predict(reading, model @each(scenario))
```

With two stations and two scenarios, this plans four jobs, one for each `forecast[station=...,scenario=...]`. The values come from the artifacts that exist, so adding a scenario to the dataset adds its jobs.

- Only one input may broadcast a given dimension, and the driving input must not already have it.
- Broadcasting never invents a pair that the driving input lacks. If `reading` has `[station, day]` and one station-day pair is missing, no forecast is made for it.
- No source has both `station` and `scenario`, so the `dimensions` line sets their order.
- A later `@vary(scenario)` collects the forecasts again.

## Composite operations

An operation can be carried out by steps instead of a command. Its header ends in `:` and its body is indented beneath it. The body reads the inputs by their port names, and assigns each named output:

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

A call to `twice` behaves like any other call, and expands into its body's steps over the caller's products. A product the body makes for itself, such as `first`, stays private to the call: the caller can read only the outputs. A body may call another composite operation, and a selector the caller gives an input holds wherever the body reads it.

`spit check` lists the expanded steps before any data is read, and `spit dag --counts` shows the jobs each step makes:

<!-- spit-doc: output
spit check comp.spit --calls
--- comp.spit
source raw : Lines [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation clean(input: Lines) -> Lines
command clean: clean_tool {input} {@output}

operation twice(input: Lines) -> (result: Lines):
    first = clean(input)
    result = clean(first)

cleaned = twice(raw)
-->

```text
cleaned = twice(…)  line 11
  line 8  cleaned::first = clean(raw)
  line 9  cleaned = clean(cleaned::first)
```

An error inside a body is reported at the call, with the body's step beneath it. See [operations carried out by steps](../language-reference.md#operations-carried-out-by-steps).

## Stages

A stage groups the steps of one phase and gives their products a shared path:

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

- A stage is indented beneath `stage name:`, and stages nest. A product's `{@stage}` is its stage path, such as `preprocess/combine`.
- A stage can set defaults, such as `default path:` or `default check:`, for its own products. A product-specific rule still takes precedence.
- Stages do not order the jobs. Product dependencies do, so `analysis` can read `denoised` by name, and two stages may even read from each other.
- Operation names are global, including those declared inside a stage. Declare an operation in the stage that holds its calls, or at the top level, or SPIT warns.
- Sources and `use` imports belong at the top level.

`spit dag` counts the jobs in each outermost stage, and `spit-bash` can run one stage on its own with `--stage`.

## Imports

`use` brings definitions in from another `.spit` file, so a library can hold operations that many pipelines share:

<!-- spit-doc: setup
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
--- text.spit = examples/imports/text.spit
-->

```spit
use shard, sort_lines from text.spit as text
sorted = text::sort_lines(text::shard)
```

The path is relative to the importing file. `as text` gives every imported name the prefix `text::`, which also resolves a clash between two libraries. Without `as`, the names come in unqualified, and without `from`, everything the file defines is imported.

- An imported operation carries its command or body, its resources and its checks, along with the operations its body calls. An imported source carries its path rule, and its sidecar companions.
- The other file's top-level steps stay in that file, so a library adds no jobs to the pipeline that uses it.
- An unknown name, an import cycle and a name collision are each an error. Use distinct aliases for a collision, and remove the cycle.
- A recipe names an imported source with its prefix, as `text::shard`.

[Reuse definitions](../language-reference.md#reuse-definitions) gives the selection rules in full.

Next: [Datasets](datasets.md).
