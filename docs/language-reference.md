# Language reference

This is the full syntax reference for `.spit` pipelines, `.spitin` recipes, and `.spitout` inputs. See the [README](../README.md) for a quick start and the [architecture](architecture.md) doc for the internal model.

A `#` that starts a word begins a comment, as in Bash. A `#` inside a word or in quotes is kept, so `--color=#fff` and `'#run'` are ordinary arguments.

## Products and dimensions

```text
source image : Image [subject, visit, run]
source reference [subject, visit]
source testset : Data
source calibration
```

Each `source` declares a product family, not an individual file. `image[subject=A,visit=1,run=2]` identifies one artifact. Types such as `Image` are optional; product names and entity bindings identify artifacts.

A source may declare the extension its files have after its type, as an operation does for its outputs: `source events : Events .tsv [subject]`, or `source events .tsv [subject]` untyped. It completes the source's path rule; see [Extensions](#extensions).

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

Each source states the order of its own dimensions: `source bold [sub, ses, run]` puts `sub` before `ses` before `run`. Most pipelines need nothing more. Two sources that order a pair differently are an error.

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
operation summarise(days: many Series, policy: Policy) -> Summary @ min(2)
summary = summarise(reading @ vary(day), policy)
```

`@ min(2)` rejects a group with fewer than two artifacts. The [sensors walkthrough](examples.md#sensors-selectors-verification-and-two-outputs) combines a `many` input, two outputs, and `verify` in a complete plan.

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

A tool that takes a folder and a name instead of a path, and adds the extension itself, is given an output's folder with `{image.dir}`, and its file name without its extension with `{image.stem}`. Either counts as using the output. `.stem` needs the output to declare its [extension](#extensions), so that SPIT knows where the name ends:

```text
operation convert(dicom: DicomDir) -> (image: Image .nii.gz, meta: Json .json beside image)
command convert: dcm2niix -z y -b y -o {image.dir} -f {image.stem} {dicom}
```

For `derivatives/image/sub=01.nii.gz`, this passes `-o derivatives/image -f sub=01`. A file at the dataset root is in folder `.`. Only outputs have `.dir` and `.stem`, and `{@output.dir}` and `{@output.stem}` name the single unnamed output's. A placeholder with any other `.` part is an error.

Every input port has a name, and its placeholder is that name. A port is written `name`, `name: Type`, `name: many`, or `name: many Type`; a lowercase word alone, as in `operation copy(image)`, is an untyped port, and type names start with a capital letter. In `source reading : Series [station, day]`, `reading` is the product that identifies artifacts and `Series` is its type: write `operation compare(reading: Series)`, then call it with `compare(reading)`. `{@output}` is the path of a single unnamed output; its `@` marks a SPIT-supplied placeholder, while a named output such as `{wm}` uses the name in the operation declaration. `output` cannot name an input or explicit output port, and the old `{output}` spelling is an error that points to `{@output}`. A command must use every output placeholder, or its `.dir` or `.stem`, except an output written [`beside`](#files-a-tool-writes-beside-another) another; a `verify` command may use inputs only. Command templates give ordered words and arguments, not shell pipelines or redirection; an unquoted `|`, `>`, `&&`, or the like is passed to the program as an argument, and SPIT warns about it. Words are split and quoted as in Bash, and every argument is passed literally: `$` and backticks are not expanded. As in Bash, text in single quotes is literal, so `awk '{print $1}'` needs no escaping; a placeholder is filled in unquoted text or double quotes. Write `{{` or `}}` for a literal brace elsewhere. Every command is checked when the pipeline is loaded: braces and quotes must balance, placeholders must name the operation's ports, and each output must appear, as above.

Products, operations and dimensions have separate names, so a dimension may share a product's (`model @ each(model)`). A product may also share its operation's name, but `spit check` warns: name the result, as in `coregistered = coreg(mc, brain)`, so the step reads as what it makes.

The first word of a command must be an executable available on `PATH` (or an executable path). SPIT emits that command without managing its installation or loading shell functions:

```text
command process: process_tool {image} {@output}
```

## Stages

A stage groups the steps of one phase of a pipeline, such as preprocessing or analysis. Write `stage name:` at the start of a line and indent the stage's lines beneath it; the next line that is not indented ends the stage. A stage is one block: a stage name may not be opened twice, so a step that belongs to it goes inside that block, and steps may use products from a later stage. From the [stages example](../examples/stages/stages.spit):

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

A stage owns the products its steps assign. Operations and commands stay global, so one declared in a stage can be used anywhere, and product names are not prefixed: `analysis` reads `merged` by name. Sources and `use` lines belong at the top level. A `path:` line inside a stage is the default for that stage's products only; a `path product:` rule still takes precedence. `{@stage}` in a path template is the name of the product's stage. A product made outside every stage, such as a step at the top level, has none, so a default that covers it writes the stage as an [optional group](#paths), `[{@stage}/]`; SPIT's error says so.

Stages nest. A `stage` header inside a stage opens a stage within it, named by its path, such as `preprocess/combine`; a line back at the outer stage's indentation closes it. From the [nested example](../examples/stages/nested.spit):

```text
stage preprocess:
    stage clean:
        sorted = sort_lines(shard)

    stage combine:
        merged = merge(sorted @ vary(part))

    resorted = sort_lines(merged)    # in `preprocess` itself
```

The lines directly in a stage share one indentation. A nested stage without its own `path:` or `ext:` line uses the nearest one around it, and `{@stage}` gives one directory per level, as in `preprocess/combine/merged/...`.

SPIT orders stages by the products they read, so a stage needs no `after` clause. Stages must not depend on each other in a cycle, even through steps outside every stage. A nested stage is compared with its siblings, and counts toward its outer stage's place among the outer stage's siblings; a step written in an outer stage itself, like one outside every stage, passes on what it reads. `dag` counts the jobs in each outermost stage and names each job's stage, and the `.spitdag` gives each job its stage as a list of names from outermost to innermost:

```sh
cargo run -- dag examples/stages/stages.spit examples/stages/stages.spitout
```

A step outside every stage stays valid.

## Reuse definitions

Import operations and source families from another `.spit` file. The path is relative to the file containing the `use` line. An operation brings its `command`; a source brings its path rule. Imports do not bring pipeline steps.

```text
use text.spit as text

sorted = text::sort_lines(text::shard)
```

`as text` gives every imported name a prefix. Without it, `use text.spit` brings the names into the current scope. To import only a few definitions, use `use shard, sort_lines from text.spit as text`. A source imported as `text::shard` also uses that name in a recipe and a `.spitout`. SPIT reports missing names, import cycles, and name collisions.

## Paths

```text
path: results/{@product}/{@entities}.txt
path image: input/{subject}/{visit}/{run}.txt
```

`path:` sets a default; without one, outputs go to `out/{@product}/{@entities}`. `path image:` overrides it for `image`. A recipe's `path:` sets the default for sources instead; see [Recipes](#recipes). Each output of a multi-output step has its own product, so its own rule. A `path:` line inside a [stage](#stages) sets the default for that stage's products. Paths are relative to the [dataset root](#recipes): the folder a recipe's `root` line names, else the recipe's folder, or the root a `.spitout` records; `--root` overrides either.

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

For `long[sub=01]` outside every stage, that becomes `derivatives/sub-01/sub-01_long.nii.gz`; for `mc[sub=01,ses=01,run=2]` in `func`, it becomes `derivatives/sub-01/ses-01/func/sub-01_ses-01_run-2_mc.nii.gz`. A product with no dimensions can use `[{@labels}_]{@product}`. Groups work in every path rule: a default, a stage's default, a product's own rule, a source's rule in a pipeline or recipe, and a `sidecars` stem, though not in a `discover` pattern, where every directory has each dimension. A group is decided per product, before any data is read, so each product has one plain template. Groups cannot nest and must contain a placeholder that could be absent; `[[` and `]]` write literal brackets. `spit check --path-rules` and `check --json` show each product's resolved template before any data is read.

Path rules are checked when the pipeline is loaded, even for products with no resolved jobs. SPIT rejects unbalanced braces, a dimension the product does not declare outside an optional group, a dimension no product declares even inside a group, a rule that omits one of the product's dimensions (use `{@entities}`, `{@labels}`, or name each one), two products whose rules give the same path for the same entities, such as a default rule without `{@product}`, and a rule that puts files inside another product's file path, such as `in/{id}.txt/out.txt` beside `in/{id}.txt`. A path must be relative, name a file rather than end in `/`, and contain no empty, `.`, or `..` directory. Missing rules are reported by `--paths` and `--root`, and collisions between resolved artifact paths once jobs are bound. SPIT warns when two artifacts' paths differ only in letter case, such as `id=A` and `id=a`: where case is ignored, as by default on macOS and Windows, they are one file.

As in Bash, an unquoted `#` starts a comment only at the start of a word, so `--color=#fff` is one argument. A `#` that ends a word, as in `{@output}# note`, stays part of the word; SPIT warns about it, since it reads like a comment. Put a space before `#` to start a comment, or quote the text to keep it.

Place a source path beside its `source` line and a derived path beside its assignment. The default can stay near the top of the file.

### Extensions

A tool decides the format of the file it writes, so an operation can say which extension each output's file has, after its type. A multi-output operation gives one per port:

```text
operation align(moving: Image, reference: Image) -> Transform .mat
operation fit(runs: many Data) -> (weights: Weights .npz, quality: Metrics .json) @ min(2)
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
path `matrix` ends in `.txt`, but operation `align` writes `.mat`; drop the extension or use `.mat`
```

A default rule that ends with an extension while an operation, a source, or `ext:` gives its products another is the same error, said once for the rule. For a source the message says the source declares the extension, as in ``path `events` ends in `.csv`, but source `events` declares `.tsv` ``.

Extensions are optional. An operation whose tool picks the format from the output's name, such as a converter, declares none, and its path rule decides. A pipeline with no extensions and no `ext:` line resolves its paths as written.

`spit check --path-rules` shows each product's path with its extension, and where the extension is declared:

```text
  matrix (output): default derivatives/{@product}/{@entities}.mat, `.mat` from operation `align`
```

Path rules also find sources. `spit inputs recipe.spitin --root data` lists each file under `data` whose path matches a source's rule, in the pipeline or the recipe, reading entity values from its placeholders. A rule matches a file's whole path, so `responses/{region}/wave{wave}.csv` does not match `wave3.csv.bak` or `wave3.csv.1`, and files that match no rule are left out. Links to files and directories are followed. A value is read only as SPIT writes it, so a file such as `in/%41.txt`, whose value SPIT would write `A`, is skipped with a warning rather than listed under a path no job would use.

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

Later steps read it as any other output, and the `.spitdag` lists it among the job's outputs. If the tool does not write it, as when a flag turns the sidecar off, the job leaves a declared output missing.

### Sidecar files

Files that travel together, such as an image and its JSON metadata, often share a name and differ only by extension. A `sidecars` block declares such sources once, with the dimensions and the path stem they share:

```text
sidecars photo [site, visit, shot]: site-{site}/visit-{visit}/photos/shot-{shot}
    source raw_photo : Image<Photo,Captured> .raw
    source photo_gps : GpsTrack .gpx
    source photo_json : CaptureMetadata .json
```

Each member is an ordinary source with the group's dimensions, whose path is the stem and its extension, as `site-{site}/visit-{visit}/photos/shot-{shot}.gpx`; steps read it by name, as any other source. Write each member indented beneath the header as a source that declares its extension, `source name : Type .ext`, or `source name .ext` untyped. The next line that is not indented ends the block. A group with no dimensions names one set of files, as `sidecars config: config/settings`.

A block belongs at the top level of a pipeline. Its members take no dimensions or path rules of their own.

When `spit inputs` scans a dataset, it warns about each place it found some of a group's files and not the others, as `warning: photo[site=A,visit=2,shot=3] has .raw and .gpx but no .json`, before a step fails to find the missing one. A file an `exclude` rule removes is not counted as missing.

## Recipes

A `.spitin` recipe says how to find one dataset's inputs, keeping everything about the data out of the pipeline. Its first line names the pipeline it serves, relative to the recipe's folder:

```text
pipeline analysis.spit

discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
exclude image[sub=04,ses=2]    # scanner fault
drop [sub] where sessions count<2
require image count=1 per [sub, ses]
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
```

A recipe may contain `discover`, `exclude`, `drop` and `require` rules, `path product:` rules for source products, a default `path:` rule for its sources, and `sources:`/`contexts:` records. It cannot declare sources, `sidecars` groups, operations, steps, commands, stages, imports, or `ext:`; the pipeline still declares each logical `source` with its dimensions and optional type. Rules in a pipeline are an error, and so are records. A source's path rule is written in the pipeline or in the recipe, not both: put it in the pipeline when every dataset for that pipeline shares the layout, and in the recipe when the layout belongs to one dataset.

A recipe's `path:` line is the default for every source with no rule of its own, in the pipeline or the recipe. Where a dataset keeps its inputs is the dataset's to say, so the pipeline's `path:` can say where outputs go, by stage if it likes, and the recipe says where sources are:

```text
# analysis.spit
path: {@stage}/{@product}/{@entities}
source t1w : Image .nii.gz [sub]
source events .tsv [sub]

# dataset.spitin
pipeline analysis.spit
path: rawdata/sub-{sub}/{@product}
```

A source takes its rule in the pipeline, else its rule in the recipe, else the recipe's default, else the pipeline's default. A pipeline default that names `{@stage}` outside a group finds no source, since no source is made in a stage, so it covers only outputs. Each source completes the recipe's default with the extension it declares, so one default finds `rawdata/sub-01/t1w.nii.gz` and `rawdata/sub-01/events.tsv`. `ext:` completes output paths and does not apply to the recipe's default, so a source that declares no extension takes the default as written. It cannot name `{@stage}`. `spit check recipe.spitin --path-rules` lists a source it covers as `default ... (recipe)`, and the `.spitout` writes it under `source_paths:` as each such source's rule.

A recipe may also name its dataset root, the folder its paths are relative to, once:

```text
pipeline analysis.spit
root data
```

The folder is relative to the recipe's folder, like the `pipeline` line, and may use `..` or be absolute. Without the line, the root is the recipe's folder; `--root` overrides either. A pipeline has no `root` line. `spit check` warns when the folder is not there.

`spit check recipe.spitin` checks the rules against the pipeline without reading any data: each rule must name a source or discovery with the dimensions it counts, and each source path the recipe gives, by its own rule or its default, must pass the [path checks](#paths), such as telling apart the sources a default covers. `spit inputs recipe.spitin` scans the root, applies the rules, and prints the `.spitout`. A recipe that writes its own `sources:` records is not scanned unless `--root` is given; the scan then replaces them. A `root` line only says where the dataset is: it does not make the recipe's records a scan, and their files must still exist under it. `spit dag recipe.spitin` runs the same step in memory before resolving jobs, over the pipeline the recipe's `pipeline` line names. A recipe is given alone; the pipeline is not named a second time on the command line.

Three rules leave data out, each for a different reason:

| To | Write | For example |
| --- | --- | --- |
| Remove named artifacts or groups, such as a corrupted run | `exclude` | `exclude bold[sub=02,ses=02,run=3]  # corrupted` |
| Remove every group that fails a criterion, as the data changes | `drop` | `drop [sub] where sessions count<2` |
| Plan what can be completed despite missing inputs | `dag --partial` | `spit dag dataset.spitin --partial -o plan.spitdag` |
| Stop, when the data is incomplete | `require` | `require t1w count=1 per [sub, ses]` |

They apply in that order, however they are written: first every `exclude`, then every `drop`, each judged against what the exclusions leave, then every `require`, checked against what the drops leave. What `exclude` and `drop` remove is reported on stderr as notes and recorded in the `.spitout`.

Rules that count form their groups from every artifact and discovered context in the dataset, whichever source or discovery found it. `drop [store] where pricing count=0` groups by every store any source or discovery has, so a store with sales but no price list is a group with none: its count is 0.

### Discover contexts from directories

```text
discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
```

`discover` extracts global entity bindings from directories. `sessions` names the rule; it is not an artifact or an input to a step. Each matching directory contributes one `[sub=...,ses=...]` binding, including an empty directory. Values can be strings and need not be sequential. Only pairs found on disk are included; SPIT does not form a Cartesian product of subjects and sessions. The pattern is relative to the dataset root. `spit inputs` writes the bindings under `contexts sessions:`.

If a `discover` declaration matches no directories, discovery fails and names that declaration and its pattern.

Sources whose dimensions fit within the rule's dimensions expand over the observed bindings. For example, `source image [sub, ses]` expects one image per discovered pair, while `source reference [sub]` expects one per observed subject. Their `path` rules must name regular files; a missing file is an error. A source with another dimension, such as `run`, is still found by scanning its file path rule and can use `require` to check run coverage. The directory pattern must use every declared dimension, contain no other placeholders, and name a relative directory without `.` or `..` components. Values that cannot be represented faithfully in a `.spitout` are skipped with a warning.

`require` can target the name of a discovery rule directly:

```text
require sessions count>=2 per [sub]
require sessions ses=1,2 per [sub]
```

The first rule needs at least two observed session bindings per subject. The second specifically needs sessions `1` and `2`. These rules count the directories matched by `sessions`, not artifacts from a product called `sessions`. Records keep the rule name as `contexts sessions:` followed by its `[sub=...,ses=...]` records, which `spit inputs` writes.

### Constraints

```text
require image count>=2 per [subject, visit]
require reference count=1 per [subject, visit]
```

Constraints, written in a recipe, check each observed group, and fail the run if any group fails. They do not set a total subject or visit count. The count takes any comparison: `count=1`, `count!=1`, `count>=2`, `count<=2`, `count>2` or `count<2`. A rule can also require particular values in each group, alone or with a count:

```text
require image run=1,2 per [subject, visit]
```

A `require` rule is checked after every `drop` rule, against the groups they leave. A rule whose grouping finds no group at all, because nothing in the dataset has those dimensions or a `drop` removed every one, is an error: a check of nothing is not a pass.

### Drop groups that fail a criterion

The [cohort walkthrough](examples.md#cohort-discovery-exclusion-and-grouped-removal) uses `drop` to remove a subject with too few sessions and `exclude` to remove one damaged run.

`drop` removes every group that meets its condition, and reads the way it acts: the groups, then `where`, then what removes one.

```text
drop [sub] where sessions count<2
drop [sub, ses] where t1w count=0
drop [sub, ses] where bold missing run=1,2
drop [sub, ses] where bold has run=3
```

After `where` comes the source or discovery rule to count, then one condition:

- **A count,** with any comparison: `count<2` removes each group with fewer than two.
- **`missing` values:** `missing run=1,2` removes each group without a run 1 or without a run 2.
- **`has` values:** `has run=3` removes each group with a run 3.

Removing a group removes every artifact and discovered context within it, of every source. An artifact without all the group's dimensions, such as a subject's reference when only its sessions are dropped, stays; if no job then uses it, `spit artifacts` lists it as unused. Each `drop` rule is one condition, and a group is removed when any rule's condition holds. Every rule is judged against the same inventory, so writing them in another order changes nothing. A `drop` rule that would remove every group of its grouping is an error, since nothing would be left to plan.

A file a discovered context expects but lacks counts as absent, so `drop [sub, ses] where t1w count=0` removes a session whose T1w is missing, rather than failing on the missing file. Each removed group is reported on stderr, `note: dropped [sub=5] by \`drop [sub] where sessions count<2\` (line 3); found 1`, and recorded in the `.spitout`.

A `drop` rule's values name a dimension within each group, not one of its groups: `drop [store] where sales missing store=s07` is an error, since each group has one store. To remove named groups, write `exclude [store=s07]`.

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

Exclusions apply before anything else in the recipe. An excluded discovered context expects no files, an excluded file needs to exist nowhere, and a file excluded by name may lie outside every discovered context, such as a misnamed copy. `drop` and `require` rules then see what the exclusions leave.

Files whose whole paths match no source path rule are ignored while scanning. `spit inputs` counts them in a note, leaving out SPIT's own `.spit`, `.spitin`, `.spitout` and `.spitdag` files; `spit inputs dataset.spitin --unmatched` lists their paths relative to the dataset root instead of writing a `.spitout`. Required source paths must match the spelling found by the scan: `pricing/S07.json` does not satisfy `pricing/s07.json`, even on a case-insensitive filesystem. A file with a near miss in an identity value, such as `store=S07` where a job needs `store=s07`, may still match a source rule: it is then a source artifact, and `dag` and `artifacts` warn when it is unused and point to it at the failed join.

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

A `.spitout` lists a dataset's settled source identities. `spit inputs` writes one, and a dataset indexer or a person can write one too. Paths come from rules in the pipeline or, if a recipe supplies a source rule, a `source_paths:` section written once in the `.spitout`. `contexts:` names a group even when one of its required inputs is absent:

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

The folder is relative to the `.spitout`'s own folder, and may be absolute. A printed `.spitout` records no root, since where it will be kept is unknown. `dag` and `artifacts` use it as the root, so they check the source files and run commands from it without `--root`; `--root` overrides it. The line comes before every section, once. A `.spitout` without one, printed or written by hand, has no root unless `--root` gives one.

`spit inputs` also writes what the recipe's `exclude` and `drop` rules removed, each with its rule, where the rule is, how many a counting rule found, and the reason:

```text
removed:
    bold[sub=02,ses=02,run=3]
        rule: exclude bold[sub=02,ses=02,run=3]
        at: line 4
        reason: corrupted: motion spike at volume 140
    [sub=07]
        rule: drop [sub] where sessions count<2
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
