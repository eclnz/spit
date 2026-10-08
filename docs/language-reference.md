# Language reference

This is the full syntax reference for `.spit` pipelines, `.spitin` recipes, and `.spitout` inputs. For a quick lookup by keyword or construct, use the [language catalog](reference/index.md). Start with the [guided introduction](index.md) or [getting started](getting-started.md) if you are new to SPIT; the [architecture](architecture.md) page describes the internal model.

A `#` that starts a word begins a comment, as in Bash. A `#` inside a word or in quotes is kept, so `--color=#fff` and `'#run'` are ordinary arguments.

## Products and dimensions

```text
source image : Image [subject, visit, run]
source reference [subject, visit]
source testset : Data
source calibration
```

Each `source` declares a product family, not an individual file. `image[subject=A,visit=1,run=2]` identifies one artifact. Types such as `Image` are optional; product names and entity bindings identify artifacts.

A source may declare the extension its files have after its type, as an operation does for its outputs: `source events : Events .tsv [subject]`, or `source events .tsv [subject]` untyped. It completes the source's path rule; see [Extensions](#extensions). A source whose artifacts are folders rather than files ends with `/` in the same place: `source dicom : Dicom / [sub]`; see [Folders](#folders).

A source with no dimensions takes no brackets: `source testset : Data` and `source calibration` each declare one artifact, displayed by its bare name. A source with no dimensions matches every job that takes it as an input, without a selector.

An assignment introduces a derived product automatically:

```text
processed = process(image)
average = mean(processed @ vary(run))
```

The input with the most dimensions drives a normal operation and gives its outputs their dimensions, wherever it sits among the ports. Its observed artifacts determine the initial jobs: an input with `[config, seed]` creates only the config and seed pairs actually present, rather than every combination of known values. For an aggregation, `vary(run)` removes `run` from the output identity; `@ each(...)` adds a dimension, as described under selectors. You can write the output type and dimensions explicitly when helpful; SPIT checks them against the step and the [dimension order](#dimension-order):

```text
average : Image [subject, visit] = mean(processed @ vary(run))
```

### Dimension order

A pipeline has one dimension order, and every product lists its dimensions in it. The order decides how a `many` input's artifacts are sorted, so the order of their command arguments, and how `{@entities}` and displayed identities are written.

Each source states the order of its own dimensions: `source bold [sub, ses, run]` puts `sub` before `ses` before `run`. These declarations establish the pipeline's order wherever they relate its dimensions. Two sources that order a pair differently are an error.

When a product holds two dimensions that no source orders, declare the order once, anywhere at the top level:

```text
dimensions [model, config, seed]
```

This happens when `@ each` broadcasts a dimension that no source shares with the driving input's: in the [ragged sweep](examples.md#ragged-sweep-correlated-seeds-and-collection-order), `trained` holds `model` and `config`, and without the line `spit check` stops there and suggests one. A `dimensions` line names every dimension in the pipeline once, and each source must list its dimensions in that order. A step's output written with its dimensions, as in `summary : Summary [model, config] = ...`, must list them in the pipeline's order; the annotation checks the order, it does not set it.

## Operations and commands

```text
operation process(image: Image) -> Image
command process: process_tool --in {image} --out {@output}

operation mean(images: many Image) -> Image
command mean: mean_tool {images} --out {@output}
```

Declare an operation before its first use. Inputs in a call follow the port order in the declaration, and SPIT checks each product's type against that port. For example, with `operation compare(series: Series, policy: Policy)`, `compare(reading, policy)` uses `reading` as `series`; reversing the arguments is a type error when their types are known. A `one` input must resolve to exactly one artifact for each job, so every other input may only use dimensions the driving input has, unless it broadcasts them with `@ each(...)`; SPIT rejects a pipeline that breaks this before reading any inputs, and reports a missing match for a job. Every `many` input names the dimensions it collects at the call, with `@ vary(dimension, ...)`; the operation only says `many`, so one operation can collect runs in one step and sessions in another. Its command placeholder expands to one separately quoted argument per artifact, in natural order. Artifacts are compared dimension by dimension in the pipeline's [dimension order](#dimension-order). Within a value, runs of digits compare as numbers and other characters compare one by one, so `run=2` comes before `run=10`, ISO dates such as `2026-09-01` sort by date, and names sort by character (`lr-high`, `lr-low`, `warmup`). Values equal as numbers but written differently, such as `1` and `01`, are then ordered by their text. A many placeholder must occupy a whole argument. An operation takes at most one `many` input, which may sit beside `one` inputs; each of those is matched once per group:

```text
operation summarise(days: many Series @ min(2), policy: Policy) -> Summary
summary = summarise(reading @ vary(day), policy)
```

`@ min(2)` on the `many` input rejects a group with fewer than two artifacts; untyped, it is `days: many @ min(2)`. It goes beside the input it counts, not after the outputs: `-> Summary @ min(2)` is an error that gives the line rewritten. The [sensors walkthrough](examples.md#sensors-selectors-verification-and-two-outputs) combines a `many` input, two outputs, and `verify` in a complete plan.

One aggregate can remove several dimensions at once. List them in one `@ vary(...)`; their order within the clause does not change the collection order. With `summary [model, config]`, this makes one leaderboard over all model and config combinations, ordered first by model and then by config:

```text
operation leaderboard(summaries: many Summary) -> Table
board = leaderboard(summary @ vary(model, config))
```

`@ min(n)` counts the whole collection, across both dimensions. A call that writes two `@ vary` clauses is an error; put both dimensions in one clause. The collection order follows the pipeline's [dimension order](#dimension-order), even if `@ vary` lists those dimensions in another order. The [ragged sweep walkthrough](examples.md#ragged-sweep-correlated-seeds-and-collection-order) declares `dimensions [model, config, seed]` and gets a model-first collection.

Selectors narrow what an input matches. The [sensors walkthrough](examples.md#sensors-selectors-verification-and-two-outputs) shows `where` and `same` with a complete inventory:

```text
calibrated = calibrate(reading, calibration @ where(revision=2))
anomaly = compare(calibrated, reference @ same(station))
```

`where(revision=2)` keeps the artifacts with that value and takes `revision` out of matching, so a family with an extra dimension can join a less specific input. `same(station)` matches on `station` alone; the reference's other dimensions must then leave exactly one artifact for each job. Selectors can be combined, as in `frame @ where(acq=fast) @ vary(run)`.

`each` does the reverse of `vary`: it broadcasts an input over a dimension the driving input lacks, so the step runs once for every value and its outputs gain that dimension:

```text
dimensions [station, scenario]
source reading : Series [station]
source model : Model [scenario]
source parameters : Parameters [scenario]

forecast = predict(reading, model @ each(scenario), parameters)
```

With two stations and two scenarios, this makes four `forecast[station=...,scenario=...]` jobs. The values come from the artifacts of the broadcast input, so adding a scenario to the inputs adds its jobs. Other inputs are matched on the new dimension as usual; here `parameters` supplies the settings for each scenario. Only one input may broadcast a given dimension, and the driving input must not already have it. No source holds both `station` and `scenario`, so the `dimensions` line orders them, and `forecast` has dimensions `[station, scenario]`. `each` pairs with `vary`, so a sweep can be collected again. It crosses only the broadcast input's observed values with each driving artifact; values held by other inputs stay correlated through matching. The [ragged sweep walkthrough](examples.md#ragged-sweep-correlated-seeds-and-collection-order) shows models crossed with observed config/seed pairs without inventing a missing seed:

```text
trial = simulate(reading, seed @ each(rep))
summary = average(trial @ vary(rep))
```

An output's type may be followed by the extension the tool gives its file, as in `-> Transform .mat`; see [extensions](#extensions). An output the tool writes next to another without being told where is declared [`beside`](#files-a-tool-writes-beside-another) it.

An operation can write several outputs in one job. Name each output; its name is its placeholder, and the call assigns one product to each:

```text
operation estimate(dwi: DWI) -> (wm: Response, gm: Response, csf: Response)
command estimate: dwi2response dhollander {dwi} {wm} {gm} {csf}
wm_response, gm_response, csf_response = estimate(dwi)
```

A `verify` command checks a job's inputs before its command runs, using the tools that understand the files. SPIT does not run it; it writes each job's `verify` commands into the `.spitdag` beside its command, and a backend runs them first, in order. If one fails, the job does not run, and neither does any job that depends on it:

```text
verify register: check_same_grid {moving} {reference}
```

A `verify` command may use any input port, including a `many` one, which is filled in as in the command: `verify fit: validate_panel {waves}` checks every wave a fit job reads. `spit dag --commands` shows each job's `verify` lines above the command they guard, with their paths filled in.

A tool that takes a folder and a name instead of a path, and adds the extension itself, is given an output's folder with `{image.dir}`, and its file name without its extension with `{image.stem}`. Either counts as using the output. `.stem` needs the output to declare its [extension](#extensions), so that SPIT knows where the name ends, unless the output is a [folder](#folders), whose stem without one is its whole name:

```text
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -z y -b y -o {image.dir} -f {image.stem} {dicom}
```

For `derivatives/image/sub=01.nii.gz`, this passes `-o derivatives/image -f sub=01`. A file at the dataset root is in folder `.`. Only outputs have `.dir` and `.stem`, and `{@output.dir}` and `{@output.stem}` name the single unnamed output's. A placeholder with any other `.` part is an error.

Every input port has a name, and its placeholder is that name. A port is written `name`, `name: Type`, `name: many`, or `name: many Type`; a lowercase word alone, as in `operation copy(image)`, is an untyped port, and type names start with a capital letter. In `source reading : Series [station, day]`, `reading` is the product that identifies artifacts and `Series` is its type: write `operation compare(reading: Series)`, then call it with `compare(reading)`. `{@output}` is the path of a single unnamed output; its `@` marks a SPIT-supplied placeholder, while a named output such as `{wm}` uses the name in the operation declaration. `output` cannot name an input or explicit output port, and the old `{output}` spelling is an error that points to `{@output}`. A command must use every output placeholder, or its `.dir` or `.stem`, except an output written [`beside`](#files-a-tool-writes-beside-another) another; a `verify` command may use inputs only. Command templates give ordered words and arguments, not shell pipelines or redirection, since a backend runs a command without a shell. An unquoted `|`, `>`, `&&`, `;`, `2>&1` or the like is therefore an error; quote it (`'>'`) to pass it to the program as an argument. A command that needs a pipe or a redirection runs a shell itself and passes the paths to it as arguments, so that a path is never read as shell text: `command first: sh -c 'cut -f1 "$1" > "$2"' sh {table} {@output}`. The same holds for `verify` and `check` commands. Words are split and quoted as in Bash, and every argument is passed literally: `$` and backticks are not expanded. As in Bash, text in single quotes is literal, so `awk '{print $1}'` needs no escaping; a placeholder is filled in unquoted text or double quotes. Write `{{` or `}}` for a literal brace elsewhere. Every command is checked when the pipeline is loaded: braces and quotes must balance, placeholders must name the operation's ports, and each output must appear, as above.

Products, operations and dimensions have separate names, so a dimension may share a product's (`model @ each(model)`). A product may also share its operation's name, but `spit check` warns: name the result, as in `coregistered = coreg(mc, brain)`, so the step reads as what it makes.

The first word of a command must be an executable available on `PATH` (or an executable path). SPIT emits that command without managing its installation or loading shell functions:

```text
command process: process_tool {image} {@output}
```

### Operations carried out by steps

An operation may be carried out by steps instead of a command: its header ends in `:`, and its steps are indented beneath it. A call to it looks like any other call, and becomes the body's steps over the caller's products, each with its own jobs:

```text
operation clean(x: Lines, t: Table) -> Lines
command clean: clean {x} {t} {@output}
operation merge(xs: many Lines) -> Lines
command merge: merge {xs} {@output}
operation count(x: Lines) -> Count
command count: wc {x} {@output}

operation summarise(reads: Lines, table: Table) -> (merged: Lines, total: Count):
    cleaned = clean(reads, table)
    merged = merge(cleaned @ vary(lane))
    total = count(merged)

first, first_total = summarise(raw, calibration @ where(revision=2))
```

The body reads the operation's inputs, by their port names, and the products its earlier steps make, and it makes each named output once. Its outputs are the products the caller names: here `first` and `first_total`. A product the body makes for itself is filed under the call's first output, so `cleaned` is `first::cleaned`, written `first.cleaned` in a path, and a second call of `summarise` files its own apart. So a call's first output cannot also be an import's alias: `use parts.spit as first` with the call above is an error at the call. Only the outputs are the caller's to read: a step that reads `first::cleaned` is an error that says to make it an output.

A selector the caller gives an input holds wherever the body reads it, beside the body's own: above, every `clean` job reads revision 2, and `merge` collects each group's lanes. A call in a stage puts every step it makes in that stage. A body may call another operation with a body, which is expanded in turn; every operation a body calls is declared before it. An output written with a type, as `total: Count`, gives the caller's product that type, and SPIT checks it against the step that makes it. An error in a step the call makes, such as a type the step does not accept or an input the data lacks, is reported at the call, named before the message. It points at the argument the caller gave when the failing input reads one of the operation's inputs, at the product the caller names when it is one of the outputs, and at the whole call otherwise. Below it, a `-->` line gives each call it is nested in and the body's step, with their file and line:

```text
error: line 4, column 21: in `m, t = L::summarise(...)`: type mismatch at `L::clean.x`: product `cal` is Table, expected Lines
  --> libs/lib.spit: line 13, column 15: the call of `L::tidy` in the body of `L::summarise`
  --> libs/lib.spit: line 10, column 9: the step in the body of `L::tidy`
```

`spit check --json` gives the same places as the diagnostic's `related` list. `spit artifacts` names the call beside each artifact a call's step cannot make, as ``(L::clean, in `m, t = L::summarise(...)` on line 8)``, and the reasons in a partial `.spitdag`'s `left_out` start with it.

A check on an input or output of such an operation, as `reads: Lines @ check(lines(2))`, runs on every step that reads that input or makes that output, beside the checks of the step's own operation; the same check on one artifact runs once. Imported, it brings the operations its steps call, and their commands and checks, under the same prefix, so `use summarise from lib.spit as L` brings `L::clean` too.

`spit check pipeline.spit --calls` lists each call's steps before any data is read. It shows the call, its line and stage, the file that declares the operation with its file's blob id, then each step with the line of the body that writes it; a call in a body is shown under its caller:

```text
m, t = L::summarise(…)  line 6  [report]
  L::summarise  libs/lib.spit  blob 3b18e5c
  line 13  m::cleaned = L::tidy(raw, cal)
    line 10  m::cleaned = L::clean(raw, cal)
  line 14  m = L::merge(m::cleaned)
  line 15  t = L::C::count(m)
```

With `--json` the same calls are the `calls` array of `{"diagnostics":[...],"calls":[...]}`, one object per call with its `steps`, a nested call having its caller's `id` as `parent`.

`spit dag --counts` lists a call's steps under it, with the call's jobs in all, so `summarise` over two groups of two lanes shows:

```text
jobs  step
      first, first_total = summarise
   4    first::cleaned = clean
   2    first = merge
   2    first_total = count
   8    in this call
   8  total
```

`spit dag --commands` starts each job a call made with a `from:` line: each call the job is nested in, outermost first, as `first = summarise (pipeline.spit line 19)`, then the file and line of the body's step that made it. The `.spitdag` holds the same, under each job's [`origin`](spitdag.md#where-jobs-come-from).

An operation with a body names its outputs, as `-> (result: Type)`, since its steps assign them by name. An output takes its extension, folder and place from the step that writes it, so the header gives only its name and type. Such an operation takes no `command` or `verify` line; its steps' operations have their own.

## Checks

A `check` tests one artifact once its file exists, with the tools that understand it. Declare it once, then attach it with `@ check(...)` where it applies:

```text
check ndim(n): check_ndim {@path} {n}
check nonempty: test -s {@path}

source t1w : Image .nii.gz [sub] @ check(ndim(3))

operation denoise(dwi: DWI @ check(ndim(4))) -> DWI .mif @ check(nonempty)
operation split(table: Table) -> (left: Table @ check(nonempty), right: Table)
```

`{@path}` is the artifact being checked, and the check must use it. Each `{param}` is the word given where the check is attached: `ndim(4)` runs `check_ndim` with the artifact's path and `4`. A check may use nothing else, so it reads one artifact and never adds a dependency, and it must use every parameter. A check with no parameters is written without parentheses, where it is declared and where it is attached. An argument is one word, with no spaces, quotes, braces, commas or parentheses. The command is split and quoted like any [command](#operations-and-commands).

`@ check(...)` follows an input port's type, an output's type and extension, or a source's dimensions, and may name several checks, as in `@ check(nonempty, ndim(3))`. The checks on an input port, and those of the source it reads, run on each artifact the port reads before the job's `verify` commands and command; a `many` port checks each artifact of its collection. The checks on an output run after the command, once the file exists, before the job counts as done. Several checks on one artifact all run; none replaces another. A step's product takes no checks: attach them to the operation's output.

SPIT does not run checks. It writes each job's checks into the `.spitdag`, bound to the artifacts they test (see [checks](spitdag.md#checks)), and a backend runs them. A failed check fails the job, even when its command succeeded, and the jobs that depend on it do not run. When the job that writes an artifact checks it after its command, a job in the same plan that reads it does not run the same check again. `spit dag --commands` shows each job's checks as `check:` lines, in the order they run.

Checks are global, as operations are. `use` brings in the checks of the operations and sources it imports, and `use ndim from checks.spit` imports a check by name. With `as`, an imported check takes the prefix too, as in `@ check(img::ndim(3))`; see [reuse](#reuse-definitions).

### Default checks

When every output in a file or stage needs the same check, write one `check:` line instead of repeating `@ check(...)` on each output, as `path:` and `ext:` set a default for the products they cover:

```text
check nonempty: test -s {@path}
check ndim(n): check_ndim {@path} {n}

check: nonempty

stage preprocess:
    check: ndim(3)
    cleaned = denoise(raw)
```

A `check:` line outside every stage lists the checks run on every output of every step in it, stages included; one inside a stage lists them for the steps of that stage and the stages nested in it. A step takes the defaults of its own stage, not of the stage where its operation was declared, so a global operation called in two stages is checked by each stage's list. A default applies to every output of the step, each artifact of a named multi-output operation included. It never applies to an input port or a source: those keep the checks written at their `@ check(...)`.

Lists add up. A step's outputs run the file's checks, then those of each stage around the step from the outermost in, in the order written, and then the checks the operation's own output names. Where one check would run twice on an artifact, it runs once, at its first place. In the example above, `cleaned` runs `nonempty`, then `ndim(3)`.

To run less than a wider list sets, write `!` before the check. In a stage, `check: !nonempty` drops the file's `nonempty` for that stage's outputs and the stages in it, and a stage inside it may add it back. On an operation's output, `-> (empty_ok: Table @ check(!nonempty), rows: Table)` drops it for that output alone, wherever the operation is called. `!` names a check as written, `ndim(3)` included, and an output's own `@ check(nonempty)` is never dropped by it.

A file or a stage has one `check:` line. Its checks and the ones it drops must be declared checks, with the right number of arguments. A product may still be named `check`: `check = clean(raw)` and `check : Table [id] = clean(raw)` are steps, since a `check:` line has a list and no `=` outside parentheses. The `.spitdag` has no new fields: each default becomes a check on the artifact, after the job's command, beside the others, and `spit dag --commands` lists it as a `check:` line.

## Stages

A stage groups the steps of one phase of a pipeline, such as preprocessing or analysis. Write `stage name:` at the start of a line and indent the stage's lines beneath it; the next line that is not indented ends the stage. A stage is one block: a stage name may not be opened twice, so a step that belongs to it goes inside that block, and steps may use products from a later stage. From the [stages example](https://github.com/eclnz/spit/blob/dev/examples/stages/stages.spit):

```text
path: {@stage}/{@product}/{@entities}.txt

source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

stage preprocess:
    operation sort_lines(input: Lines) -> Lines
    command sort_lines: sort -u -o {@output} {input}
    sorted = sort_lines(shard)

    operation merge(items: many Lines) -> Lines
    command merge: sort -m -u -o {@output} {items}
    merged = merge(sorted @ vary(part))

stage analysis:
    path: results/{@product}/{@entities}.txt

    operation tally_lines(input: Lines) -> Tally
    command tally_lines: uniq -c {input} {@output}
    tally = tally_lines(merged)
```

A stage owns the products its steps assign. Operations and commands stay global, so one declared in a stage can be used anywhere, and product names are not prefixed: `analysis` reads `merged` by name. Declare an operation in the stage that holds its calls, or at the top level: SPIT warns about one called outside the stage it is declared in, and names the innermost stage that holds every call, else the top level. Two operations may not share a name, even in different stages. Sources and `use` lines belong at the top level. A `path:` line inside a stage is the default for that stage's products only; a `path product:` rule still takes precedence. `{@stage}` in a path template is the name of the product's stage. A product made outside every stage, such as a step at the top level, has none, so a default that covers it writes the stage as an [optional group](#paths), `[{@stage}/]`; SPIT's error says so.

Stages nest. A `stage` header inside a stage opens a stage within it, named by its path, such as `preprocess/combine`; a line back at the outer stage's indentation closes it. From the [nested example](https://github.com/eclnz/spit/blob/dev/examples/stages/nested.spit):

```text
stage preprocess:
    stage clean:
        sorted = sort_lines(shard)

    stage combine:
        merged = merge(sorted @ vary(part))

    resorted = sort_lines(merged)    # in `preprocess` itself
```

The lines directly in a stage share one indentation. A nested stage without its own `path:` or `ext:` line uses the nearest one around it, and `{@stage}` gives one directory per level, as in `preprocess/combine/merged/...`.

A stage groups steps and scopes their paths; it does not order them. SPIT orders jobs by the products they read, so a stage needs no `after` clause, and two stages may read from each other: `b` in `first` may read `a` from `second` while `second` reads `c` from `first`. A stage opened again, by a second `stage name:` header at the same level, continues the first block: its steps belong to the same stage. Its `path:` and `ext:` lines are still given once, in either block. `dag` counts the jobs in each outermost stage and names each job's stage, and the `.spitdag` gives each job its stage as a list of names from outermost to innermost:

```sh
cargo run -- dag examples/stages/stages.spit examples/stages/stages.spitout
```

A step outside every stage stays valid.

## Reuse definitions

Import operations, source families and [checks](#checks) from another `.spit` file. The path is relative to the file containing the `use` line. An operation brings its `command`; a source brings its path rule; either brings the checks it attaches. A source with companions declared [`beside`](#sidecar-files) it brings those companions with it. A companion cannot be imported alone: import its main source. A recipe names an imported main source as `text::raw_photo` when the import uses `as text`. An [operation carried out by steps](#operations-carried-out-by-steps) brings the operations its steps call. Imports do not bring pipeline steps.

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

## Paths

```text
path: results/{@product}/{@entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

`path:` sets a default; without one, outputs go to `out/{@product}/{@entities}`, which `spit check --path-rules` lists as `built-in default`. `path image:` overrides it for `image`. Sources never take the built-in path: a source with no rule needs one from a recipe or a `.spitout`'s `source_paths:`. A recipe's `path:` sets the default for sources instead; see [Recipes](#recipes). Each output of a multi-output step has its own product, so its own rule. A `path:` line inside a [stage](#stages) sets the default for that stage's products. Paths are relative to the [dataset root](#recipes): the folder a recipe's `root` line names, else the recipe's folder, or the root a `.spitout` records; `--root` overrides either.

A source with no dimensions can use a fixed path, such as `path testset: eval/testset.parquet`.

A template fills these placeholders from the artifact it names, here `aligned[subject=A,run=2]` made in stage `preprocess/align`:

| Placeholder | Expands to | Example |
| --- | --- | --- |
| `{@product}` | The product's name; an imported `alias::name` becomes `alias.name` | `aligned` |
| `{@entities}` | Every dimension as `dim=value`, in the pipeline's dimension order, joined by `__`; `global` for a product with no dimensions | `subject=A__run=2` |
| `{@stage}` | The stage whose block holds the step, one directory per level; an error for a product made outside every stage | `preprocess/align` |
| `{@labels}` | Every dimension as `key-value`, in pipeline dimension order, joined by `_`; no value for a product without dimensions | `subject-A_run-2` |
| `{subject}`, `{run}`, … | The value of a dimension the product declares | `A`, `2` |

SPIT's own placeholders take `@`, and a dimension takes none, so a dimension may be called `product` or `stage`. `{product}` with no such dimension is an error that says to write `{@product}`. Values keep letters, digits, and `-`; any other byte is written as `%` and two hex digits, so a value never adds a directory. `{@labels}` uses the dimension names as keys; use explicit text such as `sub-{subject}` when a dataset calls a dimension by another name. SPIT warns if a value written through `{@labels}` contains `-`, because a BIDS reader cannot recover that value from the file name.

A path may put text in `[...]` when only some products have it. SPIT keeps the group if every placeholder in it has a value for the product, or drops the whole group if a dimension is absent, `{@stage}` has no stage, or `{@labels}` has no dimensions. For example, the [cohort pipeline](examples.md#cohort-discovery-exclusion-and-grouped-removal) uses one default for run images, session averages, and subject averages:

```text
path: derivatives/sub-{sub}[/ses-{ses}][/{@stage}]/{@labels}_{@product}
ext: .nii.gz
```

For `long[sub=01]` outside every stage, that becomes `derivatives/sub-01/sub-01_long.nii.gz`; for `mc[sub=01,ses=01,run=2]` in `func`, it becomes `derivatives/sub-01/ses-01/func/sub-01_ses-01_run-2_mc.nii.gz`. A product with no dimensions can use `[{@labels}_]{@product}`. Groups work in every path rule: a default, a stage's default, a product's own rule, a source's rule in a pipeline or recipe, and the path of a source with companions, though not in a `discover` pattern, where every directory has each dimension. A group is decided per product, before any data is read, so each product has one plain template. Groups cannot nest and must contain a placeholder that could be absent; `[[` and `]]` write literal brackets. `spit check --path-rules` and `check --json` show each product's resolved template before any data is read.

Path rules are checked when the pipeline is loaded, even for products with no resolved jobs. SPIT rejects unbalanced braces, a dimension the product does not declare outside an optional group, a dimension no product declares even inside a group, a rule that omits one of the product's dimensions (use `{@entities}`, `{@labels}`, or name each one), two products whose rules give the same path for the same entities, such as a default rule without `{@product}`, and a rule that puts files inside another product's file path, such as `in/{id}.txt/out.txt` beside `in/{id}.txt`, or inside a [folder](#folders) a job writes. A path must be relative, name a file rather than end in `/`, and contain no empty, `.`, or `..` directory. A source no rule covers is reported by `spit check` on a recipe and by `dag`, and collisions between resolved artifact paths once jobs are bound. SPIT warns when two artifacts' paths differ only in letter case, such as `id=A` and `id=a`: where case is ignored, as by default on macOS and Windows, they are one file.

As in Bash, an unquoted `#` starts a comment only at the start of a word, so `--color=#fff` is one argument. A `#` that ends a word, as in `{@output}# note`, stays part of the word; SPIT warns about it, since it reads like a comment. Put a space before `#` to start a comment, or quote the text to keep it.

Place a source path beside its `source` line and a derived path beside its assignment. The default can stay near the top of the file.

### Extensions

A tool decides the format of the file it writes, so an operation can say which extension each output's file has, after its type. A multi-output operation gives one per port:

```text
operation align(moving: Image, reference: Image) -> Transform .mat
operation fit(runs: many Data @ min(2)) -> (weights: Weights .npz, quality: Metrics .json)
```

An extension is a `.` and letters, digits, `-` or `_`, and may have several parts, as `.nii.gz` does. An untyped output may have one too: `-> .txt`. A source declares the extension of the files it reads in the same place, after its type: `source events : Events .tsv [sub]`.

`ext:` sets the extension for operations that declare none, at the top level or in a stage, as `path:` sets the default path. Default path rules are then written without an extension:

```text
path: derivatives/{@product}/{@entities}
ext: .img
```

A product's path is its rule, completed with an extension when the rule ends without one:

1. Its own `path product:` rule takes the operation's extension, or the one a source declares, if there is one. `ext:` never applies to it, and without such an extension the rule is used as written. A recipe's default counts as each source's own rule.
2. A default rule, its stage's or the pipeline's, takes the operation's extension or the source's, else the nearest stage's `ext:`, else the pipeline's. A source with no declared extension whose files the pipeline's default rule finds takes `ext:` too.

A rule's extension is the text of its last file name after its final placeholder, from the first `.`, as `.nii.gz` in `sub-{sub}_T1w.nii.gz`. A rule that ends with the extension it would be given is left as it is, so a full BIDS-style path can keep it. A rule that ends with another is an error, since the tool writes a different file from the one the rule names:

```text
path `matrix` ends in `.txt`, but operation `align` writes `.mat`; drop the extension or use `.mat`; SPIT reads the extension from the first `.` after the last placeholder, so keep `.` out of the name before it
```

A `.` in a name after the last placeholder is read as the start of the extension too, so `out/{sub}_acq-1.5T` for an operation that writes `.csv` is this error, with `.5T` as the extension. Keep `.` out of names, as `acq-1p5T`, which takes `.csv`; a rule that writes the whole name with its extension, as `out/{sub}_acq-1.5T.csv`, is left as it is. An extension may hold digits and more than one `.`, as `.7z` and `.tar.gz` do.

A default rule that ends with an extension while an operation, a source, or `ext:` gives its products another is the same error, said once for the rule. For a source the message says the source declares the extension, as in ``path `events` ends in `.csv`, but source `events` declares `.tsv` ``.

A [folder](#folders) takes only the extension it declares, never `ext:`.

Extensions are optional. An operation whose tool picks the format from the output's name, such as a converter, declares none, and its path rule decides. A pipeline with no extensions and no `ext:` line resolves its paths as written.

`spit check --path-rules` shows each product's path with its extension, and where the extension is declared:

```text
  matrix (output): default derivatives/{@product}/{@entities}.mat, `.mat` from operation `align`
```

Path rules also find sources. With `root data`, `spit inputs recipe.spitin` lists each file under `data` whose path matches a source's rule, in the pipeline or the recipe, reading entity values from its placeholders. A rule matches a file's whole path, so `responses/{region}/wave{wave}.csv` does not match `wave3.csv.bak` or `wave3.csv.1`, and files that match no rule are left out. When a source's rule matches no file, the scan warns and names the unmatched file nearest the rule, with the text where the file and the rule part; see [Find incomplete artifacts](guide/inspection.md). To write the rules for data that already exists, `spit inputs --suggest` prints a rule for each group of files no rule matches; see [Start from the files](https://github.com/eclnz/spit/blob/dev/README.md#start-from-the-files). Links to files and directories are followed. A value is read only as SPIT writes it, so a file such as `in/%41.txt`, whose value SPIT would write `A`, is skipped with a warning rather than listed under a path no job would use.

### Shapes on a source placeholder

A placeholder in a source rule matches any text within one folder or file name, so `logs/{server}/{date}.log` reads `logs/web1/notes.log` as `date=notes`. A shape after a `:` narrows it:

```text
path log: logs/{server}/{date:date}.log
path run: raw/sub-{sub:digits}_run-{run:digits}.csv
```

| Shape | Matches | Example |
| --- | --- | --- |
| `digits` | one or more digits; leading zeros stay in the value | `07`, `120` |
| `year` | four digits, from 1900 to 2099 | `2026` |
| `date` | `YYYY-MM-DD`, a real day in a year from 1900 to 2099 | `2026-09-01` |

With `{date:date}`, `logs/web1/notes.log`, `2026-9-1.log`, `20260901.log`, `2026-09-01-final.log` and `2026-02-30.log` match no rule, so they are not source artifacts. They are counted with the other files no rule matches, and `spit inputs --unmatched` lists them. When a source then matches no file, the warning names the nearest one: `after `logs/web1/`, the file has `notes.log` where the rule has `{date:date}.log``. The set of shapes is closed; it is not a pattern language.

Where a shape may be written:

- In a source's `path` rule, in the pipeline or a recipe's `path <source>:` line or a recipe's `path:` line, and in a `.spitout`'s `source_paths:`. A recipe's `path:` is a default for sources only.
- In a `discover ... from dirs` pattern, as `discover days: [day] from dirs data/{day:date}`.
- Not in an output's rule, nor in a pipeline or stage `path:` default, which outputs take: a shape only narrows what is read, so on a rule that writes it would check nothing, and SPIT says so. A source whose rule is a pipeline default writes its shape in a rule of its own, `path <source>: ...`, or in a recipe's `path:` default, instead. `{@entities}` and `{@labels}` take no shape either; write `sub-{sub:digits}`.

A shape reads the text of a value, which holds letters, digits and `-` only, so no shape contains `.`, `_` or `/`: `2026_09_02.log` matches no `{date}` at all, and a `.` after a placeholder still starts the extension. A dimension written twice, as `{id:digits}/{id}`, has its shape in both places; give it one shape only. Two placeholders with nothing between them may not both take a shape of any length, as `{a:digits}{b:digits}` does, since where one ends is not written; `{year:year}{n:digits}` is fine, because a year is four digits, and `{date:date}-{run:digits}` has its `-`. The check also runs on each product's path once the `[...]` groups its dimensions lack are dropped: `in/{a:digits}[-{c}]{b:digits}.txt` is fine for a product with `c`, but for one without it is `in/{a:digits}{b:digits}.txt`, and SPIT rejects it, naming the product. A shaped placeholder beside an unshaped one, `{a:digits}{b}`, is allowed and splits the way unshaped neighbours do, shortest first: `123.txt` reads `a=1`, `b=23`. A file that both a shaped rule and a general rule match, such as `{date:date}.log` and `{name}.log`, is still an error naming both sources; SPIT has no precedence between rules, so shape both or name more of the path. Shaping does not tell two sources' paths apart where they are compiled: `source a [d]`, `source b [d]` with `path a: in/{d:date}.log` and `path b: in/{d:digits}.log` is rejected, since both bind `in/d.log` for the same entities. Name the two sources' dimensions differently, as `[d]` and `[n]`, as well as shaping them. A record in a `.spitout` or an `inputs` file whose value fails its rule's shape is an error, since discovery would not read its file.

### Files a tool writes beside another

Some tools write a second file beside the one they are told to write, such as the `.json` that dcm2niix writes next to its image, or the mask FSL's `bet -m` names after its brain image. Declare such an output `beside` the output it follows, with what its file name ends with in place of that output's extension:

```text
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
operation strip(t1: Image) -> (brain: Image .nii.gz, mask: Image "_mask.nii.gz" beside brain)
```

Its path is its sibling's, without the sibling's extension, then the suffix: beside `out/image/sub=01.nii.gz`, `meta` is `out/image/sub=01.json`, and beside `sub-01_brain.nii.gz`, `mask` is `sub-01_brain_mask.nii.gz`. `{@product}` in the sibling's rule stays the sibling's name, since the file is beside the sibling's.

The suffix is an extension, or quoted text of letters, digits, `.`, `-` and `_`; the output's own extension is the suffix from its first `.`. A `beside` output:

- may be left out of the command, since the tool is not told where to write it;
- has no path rule of its own; its path follows the sibling's;
- names another output of the same operation, which declares an extension and is not itself written beside another.

Later steps read it as any other output, and the `.spitdag` lists it among the job's outputs. A declared output is never optional: if the tool does not write it, the job fails as if its command had failed, and the jobs that read its outputs do not run (see [missing outputs](spitdag.md#missing-outputs)). Whether a tool writes a sidecar usually depends on how it is run, as `dcm2niix -b n` writes no `.json`, so the same flag leaves it out of every job; an operation run that way declares no `meta`. A tool that writes a file only for some data, as dcm2niix writes `.bval` and `.bvec` only for a diffusion series, is two operations, each called on the sources it fits.

### Folders

Some tools read or write a folder of files rather than one file: a DICOM series, a FreeSurfer subject, a Zarr store. A `/` after a product's type, where an extension goes, makes its artifacts folders. It may follow an extension, as in `.zarr/`:

```text
source dicom : Dicom / [sub]
path dicom: dicom/sub={sub}
operation recon(t1: Image) -> (subject: FsSubject /)
operation store(table: Table) -> Zarr .zarr/
```

A folder's path rule names the folder, without a trailing `/`. `spit inputs` finds a folder source by matching its rule against the folders under the root, and the files inside a folder it finds are read with it, so they are not listed among the files no rule matches. A file whose path matches a folder source's rule, or a folder whose path matches a file source's, is skipped with a warning that says which kind the source reads. A folder source whose rule matches no folder is warned about as a file source is, naming the nearest folder rather than the nearest file. `dag` checks that each source folder exists, as it does each source file.

A command is given a folder by its path, as a file is: `{dicom}` above is `dicom/sub=01`. `{subject.dir}` is the folder it is in, and `{subject.stem}` its name without its extension, which for a folder without one is its whole name, so a tool that takes a parent folder and a name can be given both:

```text
command recon: recon-all -i {t1} -sd {subject.dir} -s {subject.stem}
```

For `subject[sub=01]`, at `out/subject/sub=01`, this passes `-sd out/subject -s sub=01`. `spit dag --paths` shows a folder's path with a `/` after it, `spit check --path-rules` names it `(source folder)` or `(output folder)`, and the `.spitdag` gives each artifact a [`kind`](spitdag.md#artifact).

A job owns the folder it writes, so nothing else may be written in it or read from it: a path rule that puts another product's files inside a folder a job writes is an error, as is one that puts an output inside a source folder. A source may sit in a source folder, such as `dicom/sub={sub}/info.json` beside the `dicom` folder above, since no job writes either. A folder is not written [`beside`](#files-a-tool-writes-beside-another) another output, nor has an output beside it, and a source declared [`beside`](#sidecar-files) another is a file.

### Sidecar files

Files that travel together often share a name and differ by extension. Declare the main file as a source, then declare each companion `beside` it:

```spit
source raw_photo : Image<Photo,Captured> .raw [site, visit, shot]
path raw_photo: site-{site}/visit-{visit}/photos/shot-{shot}.raw
source photo_gps : GpsTrack .gpx beside raw_photo
source photo_json : CaptureMetadata .json beside raw_photo
```

The companion is an ordinary source that a step reads by name. It inherits `raw_photo`'s dimensions and path stem: `photo_gps` above reads `site-{site}/visit-{visit}/photos/shot-{shot}.gpx`. Declare the main source first. It must be a file with an extension; a companion cannot itself be the main source of another companion. A companion has no dimensions or path rule of its own. An extension such as `.json` replaces the main file's extension; a quoted suffix such as `"_mask.nii.gz"` is appended to its stem.

The path can come from a [recipe](#recipes) when the layout varies by dataset. Name the main source and give its complete file path, including its extension:

```spit
# survey.spit
source raw_photo : Image<Photo,Captured> .raw [site, visit, shot]
source photo_json : CaptureMetadata .json beside raw_photo

# dataset.spitin
pipeline survey.spit
path raw_photo: site-{site}/visit-{visit}/photos/shot-{shot}.raw
```

A recipe's default `path:` also places the main source. For `path: data/{site}/{visit}/{shot}/{@product}`, the files are `data/a/1/3/raw_photo.raw` and `data/a/1/3/raw_photo.json`. The `.spitout` records the main source's path under `source_paths:`; the companion's path is derived from it. When importing definitions, import the main source to bring all its companions. With an alias, `path text::raw_photo:` names the imported main source.

When `spit inputs` scans a dataset, or a command reads records from a recipe or `.spitout`, it warns about each identity that holds some of these sources and lacks others, as `warning: raw_photo[site=A,visit=2,shot=3] has .raw and .gpx but no .json`. Warnings follow source declaration order, then value order. A file a named or conditional `exclude` rule removes is not counted as missing, nor is one listed under `.spitout`'s `removed:` section.

A missing companion is a warning because it matters only to a step that reads it. If `brain = strip(anat)` reads only the image, it can still plan every subject; a step reading `anat_meta` fails for a subject missing its JSON file. SPIT never runs a job with an input left out. `dag --partial` plans the remaining jobs and lists the omitted ones with their reasons. A recipe may instead remove a whole subject with `exclude [sub] where anat_meta count=0`.

## Recipes

A `.spitin` recipe says how to find one dataset's inputs, keeping everything about the data out of the pipeline. A dataset that needs nothing but its folder needs no recipe: `spit dag analysis.spit --root data` scans the folder with the pipeline's own path rules. Its first line names the pipeline it serves, relative to the recipe's folder, and its `root` line the dataset folder:

```text
pipeline analysis.spit
root .

discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
exclude image[sub=04,ses=2]    # scanner fault
exclude [sub] where sessions count<2
require [sub, ses] where image count=1
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
```

A recipe may contain `discover`, `exclude` and `require` rules, `path product:` rules for source products, a default `path:` rule for its sources, and `sources:`/`contexts:` records. It cannot declare sources, operations, steps, commands, stages, imports, or `ext:`; the pipeline still declares each logical `source` with its dimensions and optional type. Rules in a pipeline are an error, and so are records.

A recipe's `path:` line is the default for every source with no rule of its own, in the pipeline or the recipe. Where a dataset keeps its inputs is the dataset's to say, so the pipeline's `path:` can say where outputs go, by stage if it likes, and the recipe says where sources are:

```text
# analysis.spit
path: {@stage}/{@product}/{@entities}
source t1w : Image .nii.gz [sub]
source events .tsv [sub]

# dataset.spitin
pipeline analysis.spit
root .
path: rawdata/sub-{sub}/{@product}
```

A source takes the recipe's default only when it has no rule of its own, and the pipeline's default only when the recipe has none; see [Which file a line belongs in](#which-file-a-line-belongs-in). A pipeline default that names `{@stage}` outside a group finds no source, since no source is made in a stage, so it covers only outputs. Each source completes the recipe's default with the extension it declares, so one default finds `rawdata/sub-01/t1w.nii.gz` and `rawdata/sub-01/events.tsv`. `ext:` completes output paths and does not apply to the recipe's default, so a source that declares no extension takes the default as written. It cannot name `{@stage}`. `spit check recipe.spitin --path-rules` lists a source it covers as `default ... (recipe)`, and the `.spitout` writes it under `source_paths:` as each such source's rule.

A recipe names its dataset root, the folder its paths are relative to, once, on the line after `pipeline` by convention:

```text
pipeline analysis.spit
root data
```

The folder is relative to the recipe's folder, like the `pipeline` line, and may use `..` or be absolute; `root .` is the recipe's own folder. The line is required, so a recipe file always says where its data is: a recipe without one is an error, and no command-line option stands in for it. A pipeline has no `root` line. `spit check` warns when the folder is not there.

`spit check recipe.spitin` checks the rules against the pipeline without reading any data: each rule must name a source or discovery with the dimensions it counts, every source must have a path rule, by the pipeline, the recipe or a default, since the scan finds each source by its rule, and each source path the recipe gives, by its own rule or its default, must pass the [path checks](#paths), such as telling apart the sources a default covers. `spit inputs recipe.spitin` scans the root, applies the rules, and prints the `.spitout`. A recipe that writes its own `sources:` records is not scanned. Its `root` line only says where the dataset is: it does not make the recipe's records a scan, and their files must still exist under it. `spit dag recipe.spitin` runs the same step in memory before resolving jobs, over the pipeline the recipe's `pipeline` line names. A recipe is given alone; the pipeline is not named a second time on the command line.

Two forms of `exclude` leave data out; `require` checks what remains:

| To | Write | For example |
| --- | --- | --- |
| Remove named artifacts or groups, such as a corrupted run | `exclude` | `exclude bold[sub=02,ses=02,run=3]  # corrupted` |
| Remove every group that meets a condition, as the data changes | `exclude` | `exclude [sub] where sessions count<2` |
| Plan what can be completed despite missing inputs | `dag --partial` | `spit dag dataset.spitin --partial -o plan.spitdag` |
| Stop, when the data is incomplete | `require` | `require [sub, ses] where t1w count=1` |

Named exclusions apply first, even when written after conditional exclusions. Every conditional exclusion then sees the same retained inventory, and all matching groups are removed together. `require` checks what remains. Each removal is reported on stderr and recorded in the `.spitout`.

Conditional `exclude` and `require` name the groups first, then `where`, then the source or discovery rule and a condition. A conditional exclusion removes each group that meets its condition; `require` stops the run unless every group meets its own. A `require` in the older order, source first and the groups after `per`, as in `require t1w count=1 per [sub, ses]`, is an error that gives the rule rewritten. An old `drop` rule is rejected with an error that shows its `exclude` replacement.

Rules that count form their groups from every artifact and discovered context in the dataset, whichever source or discovery found it. `exclude [store] where pricing count=0` groups by every store any source or discovery has, so a store with sales but no price list is a group with none: its count is 0.

### Which file a line belongs in

A `.spit` pipeline is the reusable graph: what work to do and where its results go, for any dataset. A `.spitin` recipe binds that pipeline to one dataset: where its folder is, where its sources are when the pipeline does not say, and which of its data to leave out or require. So each line belongs in one file, except a path rule:

| Line | Pipeline | Recipe |
| --- | --- | --- |
| `source`, `dimensions`, `operation`, `command`, `verify`, steps, `stage`, `use`, `ext:` | yes | no |
| `path product:` for a product a step makes | yes | no |
| `path product:` for a source that is not declared `beside` another | either one, not both | either one, not both |
| `path:`, a default | covers outputs, and sources nothing else covers | covers sources only |
| `pipeline`, `root`, `discover`, `exclude`, `require`, `sources:`, `contexts:` | no | yes |

Put a source's own rule in the pipeline when every dataset for that pipeline shares the layout, and in the recipe when the layout belongs to one dataset. A line in the wrong file is an error that says which file it belongs in, at its line:

```text
error: line 3, column 1: a step belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `discover`, `exclude` and `require` rules
error: line 3, column 13: `image` has path rules in both .spit and .spitin; keep the pipeline's if every dataset has this layout, or the recipe's if only this one does
```

A source takes the first path rule of these that it has:

1. its own `path source:` rule, from whichever file gives it;
2. the recipe's default `path:`;
3. the pipeline's default `path:`, unless it names `{@stage}` outside a group.

A rule written for one source comes before any default, and the dataset's default for its inputs comes before the pipeline's general one. An output's path never comes from the recipe. `spit check recipe.spitin --path-rules` shows which rule each source takes, and marks those the recipe gives `(recipe)`.

### Discover contexts from directories

```text
discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
```

`discover` extracts global entity bindings from directories. `sessions` names the rule; it is not an artifact or an input to a step. Each matching directory contributes one `[sub=...,ses=...]` binding, including an empty directory. Values can be strings and need not be sequential. Only pairs found on disk are included; SPIT does not form a Cartesian product of subjects and sessions. The pattern is relative to the dataset root. `spit inputs` writes the bindings under `contexts sessions:`.

If a `discover` declaration matches no directories, discovery fails and names that declaration and its pattern.

Sources whose dimensions fit within the rule's dimensions expand over the observed bindings. For example, `source image [sub, ses]` expects one image per discovered pair, while `source reference [sub]` expects one per observed subject. Their `path` rules must name regular files; a missing file is an error. A source with another dimension, such as `run`, is still found by scanning its file path rule and can use `require` to check run coverage. The directory pattern must use every declared dimension, contain no other placeholders, and name a relative directory without `.` or `..` components. Values that cannot be represented faithfully in a `.spitout` are skipped with a warning.

`require` can target the name of a discovery rule directly:

```text
require [sub] where sessions count>=2
require [sub] where sessions has ses=1,2
```

The first rule needs at least two observed session bindings per subject. The second specifically needs sessions `1` and `2`. These rules count the directories matched by `sessions`, not artifacts from a product called `sessions`. Records keep the rule name as `contexts sessions:` followed by its `[sub=...,ses=...]` records, which `spit inputs` writes.

### Constraints

```text
require [subject, visit] where image count>=2
require [subject, visit] where reference count=1
```

Constraints, written in a recipe, check each observed group, and fail the run if any group fails. They do not set a total subject or visit count. The count takes any comparison: `count=1`, `count!=1`, `count>=2`, `count<=2`, `count>2` or `count<2`. A rule can also require particular values in each group with `has`, alone or after a count, as in `require [subject, visit] where image count>=2 has run=1,2`:

```text
require [subject, visit] where image has run=1,2
```

A `require` rule is checked after conditional exclusions, against the groups they leave. A rule whose grouping finds no group at all, because nothing in the dataset has those dimensions or an exclusion removed every one, is an error: a check of nothing is not a pass.

### Exclude groups that meet a condition

The [cohort walkthrough](examples.md#cohort-discovery-exclusion-and-grouped-removal) uses conditional `exclude` to remove a subject with too few sessions and named `exclude` to remove one damaged run.

Conditional `exclude` removes every group that meets its condition: the groups, then `where`, then what removes one.

```text
exclude [sub] where sessions count<2
exclude [sub, ses] where t1w count=0
exclude [sub, ses] where bold missing run=1,2
exclude [sub, ses] where bold has run=3
```

After `where` comes the source or discovery rule to count, then one condition:

- **A count,** with any comparison: `count<2` removes each group with fewer than two.
- **`missing` values:** `missing run=1,2` removes each group without a run 1 or without a run 2.
- **`has` values:** `has run=3` removes each group with a run 3.

Removing a group removes every artifact and discovered context within it, of every source. An artifact without all the group's dimensions, such as a subject's reference when only its sessions are removed, stays; if no job then uses it, `spit artifacts` lists it as unused. Each conditional `exclude` rule has one condition, and a group is removed when any rule's condition holds. Every rule is judged against the same inventory, so writing them in another order changes nothing. A conditional `exclude` rule that would remove every group of its grouping is an error, since nothing would be left to plan.

A file a discovered context expects but lacks counts as absent, so `exclude [sub, ses] where t1w count=0` removes a session whose T1w is missing, rather than failing on the missing file. Each removed group is reported on stderr, `note: excluded [sub=5] by \`exclude [sub] where sessions count<2\` (line 3); found 1`, and recorded in the `.spitout`.

A conditional `exclude` rule's values name a dimension within each group, not one of its groups: `exclude [store] where sales missing store=s07` is an error, since each group has one store. To remove named groups, write `exclude [store=s07]`.

### Exclude named artifacts

`exclude` removes artifacts by name, such as a corrupted run or a subject who withdrew, while their files stay where they are:

```text
exclude bold[sub=02,ses=02,run=3]    # corrupted: motion spike at volume 140
exclude [sub=07]                     # withdrew consent
exclude bold[run=3]                  # run 3 dropped from the protocol
exclude calibration                  # replaced by the pipeline's own
```

A rule names a source, some values, or both, and removes every artifact whose identity includes each value it names:

- **A source with all its dimensions** names one artifact.
- **Values alone**, in brackets, name a group: every source's artifacts with those values, and every discovered context that has them. `exclude [sub=02,ses=02]` removes a whole session.
- **A source with some of its dimensions** names part of that source only: `exclude bold[sub=02]` removes that subject's BOLD runs and nothing else, so steps other sources drive still run for them.

A comment on the line is kept as the rule's reason. Values are compared as written, so `sub=2` does not match `sub=02`. An exclude that matches nothing is an error, naming any value it comes close to, so a typo or a rule the data has outgrown does not pass unnoticed. `spit check` tests each rule against the pipeline: its source must be one, and each dimension it names must be that source's, or, for values alone, some source's.

Because values are compared as written, a group removed under one spelling keeps a source filed under another. Say store `s07`'s price list was filed as `pricing/S07.json`, so no job can price its sales, and the recipe removes the store for now:

```text
exclude [store=s07]            # price list filed as S07; renamed next week
```

Given a `.spitin` recipe, `inputs`, `dag` and `artifacts` list the spellings together as they settle its inputs, so the two are seen as one store filed twice. Only ASCII letters fold, so `é` and `É` are different values with no note; and a `.spitout` given directly to `dag` or `artifacts` has no settling step, so it gets no note. The note names each spelling with the sources that have it, and `(excluded)` for one a rule removed:

```text
note: excluded [store=s07] (line 3)
note: `store` has values that differ only in ASCII letter case, which are different values to SPIT: `S07` in pricing, `s07` (excluded)
```

The note comes up before any rule too, as ``... `S07` in pricing, `s07` in sales``. It names at most three sets for a dimension, then counts the rest. `dag` then plans the other stores, and notes that the misnamed file is left over:

```text
note: 1 source artifact is used by no job: pricing[store=S07]; `spit artifacts` lists them
```

The group rule did not remove it, since `S07` is not `s07`. A second rule names it, and the note goes:

```text
exclude [store=s07]            # price list filed as S07; renamed next week
exclude pricing[store=S07]     # the same list, under the name it was filed as
```

Named exclusions apply before scanning and missing-file validation. An excluded discovered context expects no files, an excluded file needs to exist nowhere, and a file excluded by name may lie outside every discovered context, such as a misnamed copy. Conditional exclusions then see what named exclusions leave, and `require` checks the final inventory.

A placeholder in a source rule matches any text within one folder or file name unless it takes a [shape](#shapes-on-a-source-placeholder), as `{date:date}`. Without one, `logs/{server}/{date}.log` reads `logs/web1/notes.log` as `date=notes`: check the count in `note: found N source artifacts`, and leave out a file whose value does not belong with [`exclude`](#exclude-named-artifacts), a shape, or a rule that names more of its path. Files whose whole paths match no source path rule are ignored while scanning. `spit inputs` counts them in a note, naming them when there are at most three and otherwise counting them by extension, leaving out SPIT's own `.spit`, `.spitin`, `.spitout` and `.spitdag` files and any file at a path the pipeline gives one of its outputs, or inside an output folder, such as what an earlier run wrote under the root; `spit inputs dataset.spitin --unmatched` lists their paths relative to the dataset root instead of writing a `.spitout`, even if a `require` rule fails. When a `require` count fails after the scan found no files for its source, `inputs` and `dag` name the path rule used and show an unmatched file whose path contains the source name, when there is one. A recipe's `path:` is a default for sources; use `path <source>:` for one source. Required source paths must match the spelling found by the scan: `pricing/S07.json` does not satisfy `pricing/s07.json`, even on a case-insensitive filesystem. A file with a near miss in an identity value, such as `store=S07` where a job needs `store=s07`, may still match a source rule: it is then a source artifact, and `dag` and `artifacts` warn when it is unused and point to it at the failed join.

Rules can also come from a CSV file, relative to the recipe's folder, such as a lab's list of scans that failed quality control:

```text
exclude from qc/excluded.csv
```

```csv
product,sub,ses,run,reason
bold,02,02,3,"motion spike, volume 140"
,07,,,withdrew consent
```

The header names the columns: `product` and `reason` are optional, and every other column is a dimension. Each row is one rule; an empty cell leaves its column out, so a row with no `product` names a group. Fields may be quoted, as spreadsheets write them. Each row must match something, and a message about a row names the file and its line.

## Inputs

A `.spitout` lists a dataset's settled source identities. `spit inputs` writes one, and a dataset indexer or a person can write one too. Paths come from rules in the pipeline or, if a recipe supplies a source rule, a `source_paths:` section written once in the `.spitout`. A source with records needs one or the other; `dag` fails one with neither rather than guess where its files are. `contexts:` names a group even when one of its required inputs is absent:

```text
contexts:
    [subject=A,visit=1]
sources:
    image[subject=A,visit=1,run=1]
```

For one named directory discovery, `spit inputs` nests source identities under each context. A list of values in a nested dimension expands each listed product for every value:

```text
sources:
    source_lut
contexts sessions:
    [sub=01,ses=01]:
        reverse_b0, t1w
        [run=01,02]:
            raw_dwi, dwi_bvec, dwi_bval, dwi_json
```

This declares two runs of each listed DWI source. Flat `product[dimension=value,...]` records remain valid. A source path rule declared only in a recipe is written once in the `.spitout`:

```text
source_paths:
    image: data/sub-{sub}/image.nii.gz
```

The DAG can then use the rule without loading the recipe. A record names no file of its own: its source's path rule gives it.

A `.spitout` that `spit inputs -o` writes starts with the dataset root it was settled against:

```text
root ../data
```

The folder is relative to the `.spitout`'s own folder, and may be absolute. A printed `.spitout` records no root, since where it will be kept is unknown. `dag` and `artifacts` use it as the root, so they check the source files and run commands from it. The line comes before every section, once. A `.spitout` without one, printed or written by hand, has no root, so `dag` checks no source files; give it a `root` line, or run `dag` on the recipe.

`spit inputs` also writes what the recipe's `exclude` rules removed, each with its rule, where the rule is, how many a counting rule found, and the reason:

```text
removed:
    bold[sub=02,ses=02,run=3]
        rule: exclude bold[sub=02,ses=02,run=3]
        at: line 4
        reason: corrupted: motion spike at volume 140
    [sub=07]
        rule: exclude [sub] where sessions count<2
        at: line 6
        found: 1
```

The section is a record, not a rule: the records above it already leave these out, and resolving jobs removes nothing more. Scanning again rewrites it from the recipe, so a removal survives a rescan. `dag` copies it into the `.spitdag`.

`dag --partial` is a choice for one planning run, not a recipe rule. It keeps jobs with complete inputs, including aggregate jobs whose `many` input still has complete members, and writes the other outputs with their reasons in the `.spitdag`'s `left_out` array. A `many` input's `@ min(count)` is checked after incomplete members have been removed. Plain `dag` still fails on the first incomplete job and points to `artifacts` and `dag --partial`.

## Optional types

Types are additive. You can leave them out, add them to selected products and operations, or type the whole pipeline. Known mismatches fail; missing type information does not.

In operation signatures, a single capital letter such as `S` is a local type variable. Use a `$` prefix for longer names, such as `$SourceSpace` or `$Kind`. An unprefixed name such as `World` is a concrete type; every type name starts with a capital letter. Variables are allowed in operation signatures, not product declarations:

```text
operation project(sample: Frame<$Kind,$SourceSpace>, calibration: Calibration<$Kind,$SourceSpace,$TargetSpace>) -> Frame<$Kind,$TargetSpace>
```
