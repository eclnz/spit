# Datasets

A pipeline says what to make. A dataset is the set of files it is made from. This page covers how to point a pipeline at a dataset, how SPIT finds the files, and how to leave data out, require data, or wait for a person's judgement.

## Recipes

A `.spitin` recipe selects a pipeline and identifies the data for one run. The recipe in the [first run](first-run.md) uses the folder that holds it:

<!-- spit-doc: recipe
--- lines.spit = examples/runnable/lines/lines.spit
-->

```spit
pipeline lines.spit
root .
```

Both paths are relative to the recipe file. `pipeline` names the pipeline and `root` names the dataset folder. A recipe's source paths, exclusions and requirements apply to one dataset only, so the same pipeline can have a recipe for each.

- A root is declared once, in the recipe or in the pipeline. Omit it from the recipe to inherit the pipeline's, and note that declaring it in both is an error.
- `output results` gives generated files a root of their own: see [output root](#output-root).
- A pipeline that declares its own `root` and source paths needs no recipe. `spit dag lines.spit` plans it directly. A pipeline with no root can take `--root data` on the command line.
- Selection rules always belong in a recipe, however short: `exclude`, `require`, `review ... from` and `tools from`. A line in the wrong file is an error that says which file it belongs in.

`spit check dataset.spitin` checks the recipe's rules against the pipeline without reading any data. `spit inputs dataset.spitin` scans the dataset and settles the recipe's rules, and `spit dag dataset.spitin` plans the jobs.

### Source paths

A source's path rule can be in the pipeline, when every dataset has the same layout, or in the recipe's `inputs:` block, when the layout belongs to one dataset:

<!-- spit-doc: recipe
--- lines.spit
source shard : Lines [group, part]
default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)
-->

```spit
pipeline lines.spit
root .
inputs:
    shard: text/{group}/{part}.txt
```

A source takes the first rule it has of these: its own `path` rule from either file, the recipe's `default:` entry inside `inputs:`, and then the pipeline's `default path:`. Defining the same source's path in both files is an error. `spit check dataset.spitin --path-rules` shows which rule each product takes.

### Output root

Generated files go under the dataset root unless the recipe or pipeline names an `output` folder. A separate output root keeps the dataset as it was delivered, which may be read-only, and keeps two analyses of it apart:

<!-- spit-doc: output
spit dag dataset.spitin --commands
--- dataset.spitin
pipeline lines.spit
root .
output results
--- lines.spit = examples/runnable/lines/lines.spit
--- input/alpha/01.txt
-->

```text
  run:    sort -u -o output/sorted/group=alpha_part=01.txt ../input/alpha/01.txt
```

Commands run from `results`, so they name the dataset's files from there. The folder need not exist yet, because the runner makes it. A pipeline run without a recipe can take `--output-root results` instead.

## Discovery

SPIT finds source files by matching each source's path rule against the files under the root. `spit inputs` prints what it found, and notes how many artifacts it counted on standard error:

<!-- spit-doc: output
spit inputs dataset.spitin
--- dataset.spitin
pipeline examples/runnable/lines/lines.spit
root examples/runnable/lines
-->

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

A rule matches a file's whole path, so `input/{group}/{part}.txt` does not match `input/alpha/01.txt.bak`. Check the count in the note first: a rule that matches too much or too little is the most common reason for missing jobs.

- A **shape** narrows a placeholder. `input/{group}/{part:digits}.txt` reads only digits as the part, so `input/alpha/notes.txt` is not a shard. See [reading dimensions from files](artifacts.md#reading-dimensions-from-files).
- **Unmatched files** are counted in a note. `spit inputs dataset.spitin --unmatched` lists them instead of writing an inventory.
- Values are compared as written. `group=Alpha` and `group=alpha` are different groups, and `part=3` and `part=03` are different parts. SPIT notes values that differ only in letter case or leading zeros, since they are often one file filed twice.
- Folders whose names start with a `.`, such as `.git` or a runner's state, are not scanned.
- Links to files and folders are followed.

### Contexts

An `inputs:` entry named `contexts:` records groups from directories, even empty ones, so a rule can notice that a group has no files:

<!-- spit-doc: recipe
--- lines.spit = examples/runnable/lines/lines.spit
-->

```spit
pipeline lines.spit
root .
inputs:
    contexts: input/{group}
require [group] where shard count>=1
```

A context is a group SPIT has seen on disk. With a `gamma` folder under `input`, this recipe fails because `gamma` has no shards, where without the context rule SPIT would not know `gamma` exists. A directory that is absent supplies no group, and contexts never create combinations of values: SPIT keeps only the groups found on disk. A context entry neither requires files nor rejects files outside it. Write a `require` rule for every obligation.

## Exclusions

`exclude` leaves data out for a reason the recipe records. The files stay on disk, and a comment on the line is kept as the reason:

<!-- spit-doc: output
spit inputs dataset.spitin
--- dataset.spitin
pipeline examples/runnable/lines/lines.spit
root examples/runnable/lines
exclude shard[group=alpha,part=02]    # truncated upload
-->

```text
sources:
    shard[group=alpha,part=01]
    shard[group=beta,part=01]
removed:
    shard[group=alpha,part=02]
        rule: exclude shard[group=alpha,part=02]
        reason: truncated upload
note: excluded shard[group=alpha,part=02] (line 3): truncated upload
```

There are three ways to name what to remove:

- `exclude shard[group=alpha,part=02]` removes one artifact. A source with all its dimensions names one artifact, and one with some of them names part of the source, so `exclude shard[group=alpha]` removes every alpha shard.
- `exclude [group=beta]` removes a whole group: every artifact and context with `group=beta`, from every source.
- `exclude [group] where shard count<2` removes every group that meets a condition, as the data changes. After `where` comes a source, or `contexts[...]`, and then a count such as `count<2`, a `has part=3` test, or a `missing part=1,2` test. Conditional exclusions can also test whether values are present or missing.

An exclusion that matches nothing is an error, so a typo or a rule the data has outgrown does not pass unnoticed. Each removal is reported on standard error and recorded in the inventory. A rule that would remove every group is an error.

## Coverage requirements

`require` fails when the data that remains is incomplete:

<!-- spit-doc: recipe
--- lines.spit = examples/runnable/lines/lines.spit
-->

```spit
pipeline lines.spit
root .
require [group] where shard count>=2
require [group] where shard has part=01
```

The first rule demands at least two shards in each group, and the second demands part `01` in each group. A failed requirement stops planning and names the group:

<!-- spit-doc: output
spit inputs dataset.spitin || true
--- dataset.spitin
pipeline examples/runnable/lines/lines.spit
root examples/runnable/lines
require [group] where shard count>=2
-->

```text
error: source coverage for `shard` at [group=beta]: expected at least 2 artifact(s), found 1
```

SPIT applies the rules in a fixed order, whatever order they are written in: named exclusions first, then conditional exclusions, and `require` last, on what remains. A `require` that finds no group at all is an error, since a check of nothing is not a pass.

| To | Write |
| --- | --- |
| Remove a known artifact or group | `exclude shard[group=alpha,part=02]` |
| Remove every group that meets a condition | `exclude [group] where shard count<2` |
| Stop when data is incomplete | `require [group] where shard count>=2` |
| Plan what can be made despite missing data | `spit dag --partial`, a choice for one planning run |

## Human review

An `exclude` states a reason the recipe knows. A **review** is a judgement a person makes about an artifact, such as looking at a scan and finding it unusable. The pipeline declares what a review judges, and the recipe names the file that holds the verdicts.

```spit
review shard_qc
source shard : Lines [group, part] @review(shard_qc)
```

`@review(shard_qc)` goes where `@check(...)` goes: on a source, an input port or an output. On a source or an output it judges every artifact of that product for every step that reads it, and `default review: shard_qc` judges every output in a file or stage. In the recipe, `review shard_qc from qc/shards.csv` names the verdict file, relative to the recipe:

```text
group,part,verdict,reason
alpha,01,pass,
alpha,02,fail,truncated upload
```

- A row gives a `pass` or `fail` to the artifacts whose dimensions it names. An empty cell leaves its dimension out, so one row covers every value of it.
- An artifact that passes is used as usual.
- An artifact that fails, or that nobody has judged yet, is **withheld** from the steps the review covers. A review with no file yet holds back everything it judges.
- A job that reads a withheld artifact through a single input is left out. A job with a `many` input uses the members that remain, and `@min` counts those.
- The job that makes a reviewed output still runs, so the pictures and summaries a person judges can be made from every artifact.

Withheld artifacts are not incomplete, so `spit dag` succeeds and needs no `--partial`. It notes each one:

<!-- spit-doc: output
spit dag dataset.spitin --counts
--- dataset.spitin
pipeline lines.spit
root .
review shard_qc from qc/shards.csv
--- lines.spit
review shard_qc
source shard : Lines [group, part] @review(shard_qc)
default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
path shard: input/{group}/{part}.txt
operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)
operation merge(items: many Lines) -> Lines
command merge: sort -m -u -o {@output} {items}
merged = merge(sorted @vary(part))
--- qc/shards.csv
group,part,verdict,reason
alpha,01,pass,
alpha,02,fail,truncated upload
--- input/alpha/01.txt
--- input/alpha/02.txt
--- input/beta/01.txt
-->

```text
note: review `shard_qc` rejected shard[group=alpha,part=02] (qc/shards.csv line 3): truncated upload
note: review `shard_qc` holds back shard[group=beta,part=01]: no verdict yet
```

Only `alpha` part `01` is passed, so the plan holds one sort job and one merge job. A verdict file may cover more than a plan uses: a row that matches no artifact is a note, not an error. A review judges and removes nothing, and the verdicts are recorded in the plan so that a runner can say what is waiting for one. [Reviews](../language-reference.md#reviews) gives the file format and its errors.

## Inventories

An inventory, a `.spitout`, is the record of a dataset's settled inputs. `spit inputs` writes one, and a person or a dataset indexer can write one too:

<!-- spit-doc: fragment -->

```sh
spit inputs dataset.spitin -o dataset.spitout
spit dag pipeline.spit dataset.spitout
```

An inventory has sections, each optional:

- `sources:` lists the observed artifact identities.
- `contexts:` lists groups, with or without files.
- `source_paths:` carries source path rules that came from a recipe, so the plan needs no recipe.
- `removed:` and `verdicts:` record what exclusions removed and what reviews judged.
- `root` and `output` come first, when `spit inputs -o` knows where the dataset is.

A hand-written inventory can be as short as the identities:

<!-- spit-doc: spitout -->

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
```

The planner also needs a path rule for `shard`, from the pipeline or `source_paths:`. An inventory with a `root` line has `dag` check that the source files exist on disk, and one without it checks none. Rescanning rewrites the `removed:` section from the recipe, so a removal survives a rescan. The [inputs reference](../language-reference.md#inputs) gives the whole format.

Next: [Plans](plans.md).
