# Pipeline

A `.spit` file defines product families and the steps that connect them. It contains no dataset-specific source records. Names, comments, and block structure follow the [foundations](foundations.md).

## Products and dimensions

```text
source image : Image [subject, visit, run]
source reference [subject, visit]
source testset : Data
source calibration
```

Each `source` declares a product family, not an individual file. `image[subject=A,visit=1,run=2]` identifies one artifact. Types such as `Image` are optional; product names and entity bindings identify artifacts.

A source may declare the extension its files have after its type, as an operation does for its outputs: `source events : Events .tsv [subject]`, or `source events .tsv [subject]` untyped. It completes the source's path rule; see [Extensions](paths.md#extensions). A source whose artifacts are folders rather than files ends with `/` in the same place: `source dicom : Dicom / [sub]`; see [Folders](paths.md#folders).

A source with no dimensions takes no brackets: `source testset : Data` and `source calibration` each declare one artifact, displayed by its bare name. A source with no dimensions matches every job that takes it as an input, without a selector.

An assignment introduces a derived product automatically:

```text
processed = process(image)
average = mean(processed @ vary(run))
```

Derived products take their dimensions from the call. Explicit output annotations are checked against the inferred type, dimensions, and dimension order. See [matching](matching.md).


### Dimension order

A pipeline has one dimension order, and every product lists its dimensions in it. The order decides how a `many` input's artifacts are sorted, so the order of their command arguments, and how `{@entities}` and displayed identities are written.

Each source states the order of its own dimensions: `source bold [sub, ses, run]` puts `sub` before `ses` before `run`. These declarations establish the pipeline's order wherever they relate its dimensions. Two sources that order a pair differently are an error.

When a product holds two dimensions that no source orders, declare the order once, anywhere at the top level:

```text
dimensions [model, config, seed]
```

This happens when `@ each` broadcasts a dimension that no source shares with the driving input's: in the [ragged sweep](https://github.com/eclnz/spit/tree/dev/examples), `trained` holds `model` and `config`, and without the line `spit check` stops there and suggests one. A `dimensions` line names every dimension in the pipeline once, and each source must list its dimensions in that order. A step's output written with its dimensions, as in `summary : Summary [model, config] = ...`, must list them in the pipeline's order; the annotation checks the order, it does not set it.

## Stages

A stage groups the steps of one phase of a pipeline, such as preprocessing or analysis. Write `stage name:` at the start of a line and indent the stage's lines beneath it; the next line that is not indented ends the stage. A stage may be reopened at the same nesting level; its later block continues the same stage. Steps may read products from a later stage. From the [stages example](https://github.com/eclnz/spit/blob/dev/examples/stages/stages.spit):

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

A stage owns the products its steps assign. Operations and commands stay global, so one declared in a stage can be used anywhere, and product names are not prefixed: `analysis` reads `merged` by name. Declare an operation in the stage that holds its calls, or at the top level: SPIT warns about one called outside the stage it is declared in, and names the innermost stage that holds every call, else the top level. Two operations may not share a name, even in different stages. Sources and `use` lines belong at the top level. A `path:` line inside a stage is the default for that stage's products only; a `path product:` rule still takes precedence. `{@stage}` in a path template is the name of the product's stage. A product made outside every stage, such as a step at the top level, has none, so a default that covers it writes the stage as an [optional group](paths.md#paths), `[{@stage}/]`; SPIT's error says so.

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
