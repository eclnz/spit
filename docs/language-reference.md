# Language reference

This is the full syntax reference for `.spit` pipelines, `.spitin` recipes, and `.spitout` inputs. See the [README](../README.md) for a quick start and the [architecture](architecture.md) doc for the internal model.

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

The input with the most dimensions drives a normal operation and gives its outputs their dimensions, wherever it sits among the ports. For an aggregation, `vary(run)` removes `run` from the output identity; `@ each(...)` adds a dimension, as described under selectors. You can write the output type and dimensions explicitly when helpful:

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

Declare an operation before its first use. Inputs in a call follow the port order in the declaration. A `one` input must resolve to exactly one artifact for each job, so every other input may only use dimensions the driving input has, unless it broadcasts them with `@ each(...)`; SPIT rejects a pipeline that breaks this before reading any inputs, and reports a missing match for a job. A `many` input needs `@ vary(dimension)`, and its command placeholder expands to one separately quoted argument per artifact, ordered by the product's dimensions with numbers compared as numbers, so `run=2` comes before `run=10`. A many placeholder must occupy a whole argument. An operation takes at most one `many` input, which may sit beside `one` inputs; each of those is matched once per group:

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

`each` does the reverse of `vary`: it broadcasts an input over a dimension the driving input lacks, so the step runs once for every value and its outputs gain that dimension:

```text
source reading : Series [station]
source model : Model [scenario]
source parameters : Parameters [scenario]

forecast = predict(reading, model @ each(scenario), parameters)
```

With two stations and two scenarios, this makes four `forecast[station=...,scenario=...]` jobs. The values come from the artifacts of the broadcast input, so adding a scenario to the inputs adds its jobs. Other inputs are matched on the new dimension as usual; here `parameters` supplies the settings for each scenario. Only one input may broadcast a given dimension, and the driving input must not already have it. `each` pairs with `vary`, so a sweep can be collected again:

```text
trial = simulate(reading, seed @ each(rep))
summary = average(trial @ vary(rep))
```

An operation can write several outputs in one job. Name each output; its name is its placeholder, and the call assigns one product to each:

```text
operation estimate(dwi: DWI) -> (wm: Response, gm: Response, csf: Response)
command estimate: dwi2response dhollander {dwi} {wm} {gm} {csf}
wm_response, gm_response, csf_response = estimate(dwi)
```

A `verify` command checks a job's inputs before its command runs, using the tools that understand the files; if it fails, the job does not run:

```text
verify register: check_same_grid {moving} {reference}
```

Input port names are optional. A port written as a lowercase word alone, as in `operation copy(image)`, is named `image` and untyped; type names start with a capital letter. An unnamed single input is `{input}`; multiple unnamed inputs are `{input1}`, `{input2}`, and so on. An operation whose only input is a `many` input can also reach it as `{inputs}`, whatever its name. Named ports give clearer errors, although errors also name the product bound to a port. `{output}` is the path of a single unnamed output, so `output` cannot name an input port. A command must use every output placeholder; a `verify` command may use inputs only. Command templates give ordered words and arguments, not shell pipelines or redirection; an unquoted `|`, `>`, `&&`, or the like is passed to the program as an argument, and SPIT warns about it. Words are split and quoted as in Bash, and every argument is passed literally: `$` and backticks are not expanded. As in Bash, text in single quotes is literal, so `awk '{print $1}'` needs no escaping; a placeholder is filled in unquoted text or double quotes. Write `{{` or `}}`, or `\{` and `\}`, for a literal brace elsewhere. Every command is checked when the pipeline is loaded: braces and quotes must balance, placeholders must name the operation's ports, and `{output}` must appear.

The first word of a command must be an executable available on `PATH` (or an executable path). SPIT emits that command without managing its installation or loading shell functions:

```text
command process: process_tool {image} {output}
```

## Stages

A stage groups the steps of one phase of a pipeline, such as preprocessing or analysis. Write `stage name:` at the start of a line and indent the stage's lines beneath it; the next line that is not indented ends the stage. From the [stages example](../examples/stages/stages.spit):

```text
path: {stage}/{product}/{entities}.txt

source shard : Lines [group, part]
path shard: input/{group}/{part}.txt

stage preprocess:
    operation sort_lines(input: Lines) -> Lines
    command sort_lines: sort -u -o {output} {input}
    sorted = sort_lines(shard)

    operation merge(items: many Lines) -> Lines @ drop(part)
    command merge: sort -m -u -o {output} {items}
    merged = merge(sorted @ vary(part))

stage analysis:
    path: results/{product}/{entities}.txt

    operation tally_lines(input: Lines) -> Tally
    command tally_lines: uniq -c {input} {output}
    tally = tally_lines(merged)
```

A stage owns the products its steps assign. Operations and commands stay global, so one declared in a stage can be used anywhere, and product names are not prefixed: `analysis` reads `merged` by name. Sources and `use` lines belong at the top level. A `path:` line inside a stage is the default for that stage's products only; a `path product:` rule still takes precedence. `{stage}` in a path template is the name of the product's stage.

Stages nest. A `stage` header inside a stage opens a stage within it, named by its path, such as `preprocess/combine`; a line back at the outer stage's indentation closes it. From the [nested example](../examples/stages/nested.spit):

```text
stage preprocess:
    stage clean:
        sorted = sort_lines(shard)

    stage combine:
        merged = merge(sorted @ vary(part))

    resorted = sort_lines(merged)    # in `preprocess` itself
```

The lines directly in a stage share one indentation. A nested stage without its own `path:` line uses the nearest one around it, and `{stage}` gives one directory per level, as in `preprocess/combine/merged/...`.

SPIT orders stages by the products they read, so a stage needs no `after` clause. Stages must not depend on each other in a cycle, even through steps outside every stage. A nested stage is compared with its siblings, and counts toward its outer stage's place among the outer stage's siblings; a step written in an outer stage itself, like one outside every stage, passes on what it reads. `dag` counts the jobs in each outermost stage and names each job's stage, and the `.spitdag` gives each job its stage as a list of names from outermost to innermost:

```sh
cargo run -- dag examples/stages/stages.spit examples/stages/stages.spitout
```

Stages are written in the flow form; a sectioned document cannot declare them. A step outside every stage stays valid.

## Reuse definitions

Import operations and source families from another `.spit` file. The path is relative to the file containing the `use` line. An operation brings its `command`; a source brings its path rule. Imports do not bring pipeline steps.

```text
use text.spit as text

sorted = text::sort_lines(text::shard)
```

`as text` gives every imported name a prefix. Without it, `use text.spit` brings the names into the current scope. To import only a few definitions, use `use shard, sort_lines from text.spit as text`. A source imported as `text::shard` also uses that name in a recipe and a `.spitout`. SPIT reports missing names, import cycles, and name collisions.

## Paths

```text
path: results/{product}/{entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

`path:` sets a default; without one, outputs go to `out/{product}/{entities}`. `path image:` overrides it for `image`. Each output of a multi-output step has its own product, so its own rule. A `path:` line inside a [stage](#stages) sets the default for that stage's products. Paths are relative to `SPIT_ROOT`.

A template fills these placeholders from the artifact it names, here `aligned[subject=A,run=2]` made in stage `preprocess/align`:

| Placeholder | Expands to | Example |
| --- | --- | --- |
| `{product}` | The product's name; an imported `alias::name` becomes `alias.name` | `aligned` |
| `{entities}` | Every dimension as `dim=value`, in declared order, joined by `__`; `global` for a product with no dimensions | `subject=A__run=2` |
| `{stage}` | The stage whose block holds the step, one directory per level; an error for a product made outside every stage | `preprocess/align` |
| `{subject}`, `{run}`, … | The value of a dimension the product declares | `A`, `2` |

`product`, `entities`, and `stage` are reserved: no product may declare a dimension with one of those names. Values keep letters, digits, and `-`; any other byte is written as `%` and two hex digits, so a value never adds a directory.

Path rules are checked when the pipeline is loaded, even for products with no resolved jobs. SPIT rejects unbalanced braces, a dimension the product does not declare, a rule that omits one of the product's dimensions (use `{entities}` or name each one), two products whose rules give the same path for the same entities, such as a default rule without `{product}`, and a rule that puts files inside another product's file path, such as `in/{id}.txt/out.txt` beside `in/{id}.txt`. A path must be relative, name a file rather than end in `/`, and contain no empty, `.`, or `..` directory. Missing rules are reported by `--paths` and `--root`, and collisions between resolved artifact paths once jobs are bound. SPIT warns when two artifacts' paths differ only in letter case, such as `id=A` and `id=a`: where case is ignored, as by default on macOS and Windows, they are one file.

As in Bash, an unquoted `#` starts a comment only at the start of a word, so `--color=#fff` is one argument. A `#` that ends a word, as in `{output}# note`, stays part of the word; SPIT warns about it, since it reads like a comment. Put a space before `#` to start a comment, or quote the text to keep it.

Place a source path beside its `source` line and a derived path beside its assignment. The default can stay near the top of the file.

Path rules also find sources. `spit inputs recipe.spitin --root data` lists each file under `data` whose path matches a source's rule, in the pipeline or the recipe, reading entity values from its placeholders. Links to files and directories are followed. A value is read only as SPIT writes it, so a file such as `in/%41.txt`, whose value SPIT would write `A`, is skipped with a warning rather than listed under a path no job would use.

## Recipes

A `.spitin` recipe says how to find one dataset's inputs, keeping everything about the data out of the pipeline. Its first line names the pipeline it serves, relative to the recipe's folder:

```text
pipeline analysis.spit

discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
skip sessions count>=2 per [sub]
require image count=1 per [sub, ses]
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
```

A recipe may contain `discover`, `require` and `skip` rules, `path product:` rules for source products, and `sources:`/`contexts:` records. It cannot declare sources, operations, steps, commands, stages, imports, or a default `path:` rule; the pipeline still declares each logical `source` with its dimensions and optional type. Rules in a pipeline are an error, and so are records.

`spit check recipe.spitin` checks the rules against the pipeline without reading any data: each rule must name a source or discovery with the dimensions it counts. `spit inputs recipe.spitin` scans the recipe's folder, or `--root`, applies `skip`, checks `require`, and prints the `.spitout`. A recipe that writes its own `sources:` records is not scanned unless `--root` is given; the scan then replaces them. `spit dag recipe.spitin` runs the same step in memory before resolving jobs, over the pipeline the recipe's `pipeline` line names. A recipe is given alone; the pipeline is not named a second time on the command line.

### Discover contexts from directories

```text
discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
```

`discover` extracts global entity bindings from directories. `sessions` names the rule; it is not an artifact or an input to a step. Each matching directory contributes one `[sub=...,ses=...]` binding, including an empty directory. Values can be strings and need not be sequential. Only pairs found on disk are included; SPIT does not form a Cartesian product of subjects and sessions. The pattern is relative to the recipe's folder, or to `--root` when given. `spit inputs` writes the bindings under `contexts sessions:`.

If a `discover` declaration matches no directories, discovery fails and names that declaration and its pattern.

Sources whose dimensions fit within the rule's dimensions expand over the observed bindings. For example, `source image [sub, ses]` expects one image per discovered pair, while `source reference [sub]` expects one per observed subject. Their `path` rules must name regular files; a missing file is an error. A source with another dimension, such as `run`, is still found by scanning its file path rule and can use `require` to check run coverage. The directory pattern must use every declared dimension, contain no other placeholders, and name a relative directory without `.` or `..` components. Values that cannot be represented faithfully in a `.spitout` are skipped with a warning.

`require` can target the name of a discovery rule directly:

```text
require sessions count>=2 per [sub]
require sessions ses=1,2 per [sub]
```

The first rule needs at least two observed session bindings per subject. The second specifically needs sessions `1` and `2`. These rules count the directories matched by `sessions`, not artifacts from a product called `sessions`. Records keep the rule name as `contexts sessions:` followed by its `[sub=...,ses=...]` records, which `spit inputs` writes.

Use `skip` with the same count or value clauses to omit a group instead of failing it:

```text
skip sessions count>=2 per [sub]
```

If subject `5` has one session, SPIT removes that subject's contexts and source artifacts before checking expected source files. A later `require` rule sees the remaining groups. Skipped groups are reported on stderr, so standard output stays the `.spitout`.

### Constraints

```text
require image count>=2 per [subject, visit]
require reference count=1 per [subject, visit]
```

Constraints, written in a recipe, check each observed group. They do not set a total subject or visit count. A rule can also require particular values in each group, alone or with a count:

```text
require image run=1,2 per [subject, visit]
```

## Inputs

A `.spitout` lists a dataset's settled inputs: each source artifact, by product and entity values. `spit inputs` writes one, and a dataset indexer or a person can write one too. A `.spitout` from `spit inputs` begins with `pipeline analysis.spit`, the pipeline it was made for, relative to the `.spitout`'s own folder; `spit dag` and `spit artifacts` then take the file alone, and refuse a pipeline named beside it. A `.spitout` with no `pipeline` line takes its pipeline on the command line. A record may end with `: path`, its file relative to the dataset root; `spit inputs` writes one for each file it finds, and a record without one takes its product's path rule. `contexts:` names a group even when one of its required inputs is absent:

```text
contexts:
    [subject=A,visit=1]
sources:
    image[subject=A,visit=1,run=1]: raw/A/1/run-1.nii.gz
```

## Optional types

Types are additive. You can leave them out, add them to selected products and operations, or type the whole pipeline. Known mismatches fail; missing type information does not.

In operation signatures, a single capital letter such as `S` is a local type variable. Use a `$` prefix for longer names, such as `$SourceSpace` or `$Kind`. An unprefixed name such as `World` is a concrete type; every type name starts with a capital letter. Variables are allowed in operation signatures, not product declarations:

```text
operation project(sample: Frame<$Kind,$SourceSpace>, calibration: Calibration<$Kind,$SourceSpace,$TargetSpace>) -> Frame<$Kind,$TargetSpace>
```

## Grouped sections

SPIT also accepts grouped `products:`, `operations:`, and `pipeline:` sections in a pipeline, and a `constraints:` section of `require` and `skip` rules in a recipe, as an alternative to the flow style used elsewhere in this reference. The flow style is intended for writing a pipeline in the order you read it.
