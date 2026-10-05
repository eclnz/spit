# SPIT — Simple Pipeline in Text

<img src="logo.png" alt="SPIT logo" width="160">

Real datasets are irregular: a subject with a missing scan, a station with three sensors instead of two, a folder that grows every week. Hand-written shell scripts and `for` loops turn every irregularity into a special case, and a missing input usually surfaces as a cryptic failure partway through a long run rather than up front.

SPIT separates the pipeline from the data. You describe the pipeline once — its steps, and how each one's inputs and outputs relate along dimensions such as subject, run, or visit — without listing actual files. Point that pipeline at an inventory of what inputs actually exist (a file, a directory scan, or a list you supply), and SPIT works out exactly which jobs that produces, validates the whole thing before anything runs (unresolvable dimensions, unknown placeholders, colliding output paths, and more), and can report precisely which artifacts it can and can't produce and why. It then writes every job, with its files and its command, to a `.spitdag` that a backend can run — no daemon or runtime engine to run alongside it, just the commands you already use.

Add or remove inputs and the same pipeline definition produces the right jobs, with no edits.

## Contents

- [Try it](#try-it)
- [The three steps and their files](#the-three-steps-and-their-files)
- [CLI commands and options](#cli-commands-and-options)
- [Write a pipeline](#write-a-pipeline)
- [Supply the inputs](#supply-the-inputs)
- [Where files live](#where-files-live)
- [Resolve jobs](#resolve-jobs)
- [Language reference](#language-reference)
- [More examples](#more-examples)
- [How SPIT works](#how-spit-works)
- [Documentation](#documentation)
- [Development](#development)
- [Contributing](#contributing)

## Try it

Requires a [Rust toolchain](https://www.rust-lang.org/tools/install) (stable, via `cargo`). From this repository:

```sh
cargo run -- check examples/commands/command_demo/command_demo.spit
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout -o command_demo.spitdag
```

Use `cargo build` to get the `target/debug/spit` executable. With `cargo run`, the `--` separates Cargo's arguments from SPIT's arguments.

Live validation in VS Code is maintained in the separate `spit-vscode` repository.

## The three steps and their files

SPIT runs in three steps. Each is one command, and each reads the files the previous step wrote:

| Step | Command | Reads | Writes |
| --- | --- | --- | --- |
| 1. Compile | `spit check` | a `.spit` pipeline, or a `.spitin` recipe | nothing: it reports errors and warnings |
| 2. Build inputs | `spit inputs` | a `.spitin` recipe and its pipeline, or a `.spit` pipeline and `--root`, and the dataset folder | a `.spitout` |
| 3. Resolve jobs | `spit dag`, `spit artifacts` | a `.spit` pipeline and a `.spitout` | a `.spitdag` |

| File | Holds |
| --- | --- |
| `.spit` | A pipeline: sources, operations, steps, commands, and path rules. No dataset appears in it. |
| `.spitin` | A recipe for a dataset's inputs: the pipeline it serves, its dataset root, and its `discover`, `exclude` and `require` rules and source paths. |
| `.spitout` | A dataset's settled inputs: each source artifact, with its file, and the root they were found under. |
| `.spitdag` | The resolved jobs, each with its artifacts' files and its command, as JSON, in an order they can run in: all a backend needs to run them, with the dataset folder, the programs the commands need, and a fingerprint of each job's work to tell when it must run again. |

The pipeline and the recipe split along one line. A `.spit` pipeline is the reusable graph: what work to do and where its results go, for any dataset. A `.spitin` recipe binds that pipeline to one dataset: where its folder is, where its sources are when the pipeline does not say, and which of its data to leave out or require. Each line belongs in one of the two, except `path`, which can say where a source is from either; see [Which file a line belongs in](docs/language-reference.md#which-file-a-line-belongs-in).

A later step may also take an earlier step's input and run that step in memory: `dag` and `artifacts` take a `.spitin` in place of the `.spitout`. A `.spitin` names its own pipeline, so it is given alone: `spit dag dataset.spitin`. Giving a `.spit` beside it is an error, so the two cannot disagree. A `.spitout` names no pipeline, so it takes one: `spit dag analysis.spit dataset.spitout`.

A recipe is for when a dataset needs more than its folder: rules to find, check or leave out its inputs, or source paths of its own. When the pipeline's path rules already find every source, skip it and name the folder: `spit dag analysis.spit --root data`. That runs `spit inputs` in memory on the pipeline alone, so it takes no `discover`, `exclude` or `require` rules. `--root` is taken only this way: a recipe and a `.spitout` each say where their data is with a `root` line.

SPIT itself runs nothing. A backend runs the `.spitdag`: [spit-bash](https://github.com/eclnz/spit-bash) runs its jobs on one machine, as in `spit-bash run dataset.spitin -j 4`, and takes the same files as `spit dag`.

## CLI commands and options

```text
spit check <pipeline.spit | recipe.spitin | inputs.spitout> [--path-rules] [--calls] [--json] [--stdin] [--hovers]
spit inputs <recipe.spitin> [--unmatched | --suggest | -o <file>]
spit inputs <pipeline.spit> --root <directory> [--unmatched | --suggest | -o <file>]
spit dag <recipe.spitin> [--paths | --jobs] [--commands] [--counts] [--partial] [--json | -o <file>]
spit dag <pipeline.spit> <inputs.spitout | -> [--paths | --jobs] [--commands] [--counts] [--partial] [--json | -o <file>]
spit dag <pipeline.spit> --root <directory> [--paths | --jobs] [--commands] [--counts] [--partial] [--json | -o <file>]
spit artifacts <recipe.spitin>
spit artifacts <pipeline.spit> <inputs.spitout | ->
spit artifacts <pipeline.spit> --root <directory>
```

Files come first; options follow them. `spit help` lists the commands, and `spit help <command>` or `spit <command> --help` gives one command's options.

| Command | Result |
| --- | --- |
| `check` | Compile a pipeline and report every problem the text shows, reading no data. Given a recipe, check its rules against the pipeline its `pipeline` line names. Given a `.spitout`, check the syntax of its records; `dag` and `artifacts` check them against a pipeline. |
| `inputs` | Scan the dataset folder with a recipe, apply its `exclude` rules, check its `require` rules, and print the `.spitout` with a record of what was removed. It writes nothing if a `require` rule fails. |
| `dag` | Resolve the jobs, and print each with its artifacts and dependencies. With `-o`, write them as a `.spitdag`. |
| `artifacts` | List every concrete artifact the inputs yield: the complete ones, then the incomplete ones with why each cannot be produced. Unlike `dag`, it does not stop at a missing, ambiguous, or too-small input or a coverage gap; see [Find incomplete artifacts](#find-incomplete-artifacts). |

| Option | Effect |
| --- | --- |
| `--root <directory>` | With a `.spit` pipeline given alone to `inputs`, `dag` or `artifacts`, the dataset folder to scan with the pipeline's own path rules, relative to where `spit` runs. A recipe or `.spitout` names its root with a `root` line instead, and `--root` with either is an error. |
| `-o <file>`, `--output <file>` | With `inputs`, write the `.spitout` to the file instead of standard output. With `dag`, write the `.spitdag`. |
| `--path-rules` | With `check`, list the path rule each product uses (its own, a stage's or the pipeline's default, the recipe's, or for an output the built-in `out/{@product}/{@entities}`), with any [extension](docs/language-reference.md#extensions) added to it and where that is declared. |
| `--calls` | With `check` on a pipeline, list each call to an [operation carried out by steps](docs/language-reference.md#operations-carried-out-by-steps) before the final `Pipeline valid.`, with the steps it expands to and no data read: the call as written, its line and stage, the operation's file and the first seven characters of that file's git blob id, then each step with the line of the library's body that writes it. A call in a body is shown under its caller, with its own steps beneath it. With `--json`, print `{"diagnostics":[...],"calls":[...]}` instead of the usual `check --json` output: one entry per call, in the order the pipeline's calls are made, with `id`, `parent` (the `id` of the call it is nested in, or `null`), `operation`, `outputs`, `inputs`, `line`, `stage`, `file`, `blob` and `steps`, each step with its `line`, `operation`, `outputs` and `inputs`; `calls` is left out when the pipeline has errors. It cannot combine with `--path-rules` or `--hovers`, and a recipe or `.spitout` has no calls to list. |
| `--unmatched` | With `inputs`, list files under the dataset root that match no source path rule, one per line, leaving out files at the pipeline's output paths, instead of writing a `.spitout`. |
| `--suggest` | With `inputs`, print `source` and `path` lines for the files under the dataset root that match no source path rule, instead of writing a `.spitout`; see [Start from the files](#start-from-the-files). |
| `--paths` | With `dag`, print the file under every artifact. |
| `--counts` | With `dag`, print how many jobs each step resolves instead of the jobs: one row per step, as `cleaned = clean`, with its stage when the pipeline has stages, then the total. A step that resolves no jobs shows `0`, so an empty step or an unexpected expansion stands out before the plan is run. The steps a call to an [operation carried out by steps](docs/language-reference.md#operations-carried-out-by-steps) makes are indented under the call, with the call's jobs in all. With `--commands` or `--paths`, print the counts before the jobs. With `-o`, print the counts and write the `.spitdag` too. It cannot be combined with `--json`, which prints the `.spitdag` itself. |
| `--commands` | With `dag`, print each job's checks, `verify` and command lines with their paths filled in, quoted as a shell reads them, so a line can be pasted into a shell run from the dataset folder. A job a call to an operation carried out by steps made starts with a `from:` line naming the call and the body's step. This is what plain `dag` prints. With `--paths`, print them under each job's artifacts. With `-o`, print the commands and write the `.spitdag` too, so the plan checked is the plan saved. |
| `--jobs` | With `dag`, list each job's operation, stage, and input and output artifacts with their types, in place of its commands. A pipeline whose operations have no `command` lines plans with this view; plain `dag` notes when no job has a command. |
| `--by-target` | With `artifacts`, group the incomplete artifacts by final target: each incomplete artifact no other incomplete job needs, with the incomplete artifacts it waits on nested under it, and each artifact and its reasons written once in the whole report. The complete artifacts are counted, not listed. |
| `--partial` | With `dag`, plan jobs whose inputs can be completed and record the artifacts left out of the `.spitdag`. A `many` input uses its complete members. Without it, `dag` stops at an incomplete job. |
| `--json` | With `dag`, print the `.spitdag`. With `check`, print diagnostics as JSON for editor use and stop, succeeding whatever they report. Each diagnostic has a `severity` of `error` or `warning`; those tied to a declaration, call, rule, command, or path include its `line`, and a `column` and `end_column` for the text it is about, such as one input of a call or one `{placeholder}`. Columns are 1-based and count UTF-16 code units, as editors do; `end_column` is one past the last character. When checking a recipe finds an error in its pipeline, the diagnostic includes `file` and positions in that pipeline. An error in a step a call to an [operation carried out by steps](docs/language-reference.md#operations-carried-out-by-steps) makes is placed at the call, and its `related` list gives each place in the body it comes from, each with its `line`, `column`, `end_column`, a `message`, and the `file` when it is another file, such as a library. For a pipeline that checks clean, a `paths` list gives each product whose path no rule writes in full, with its `line` and its `path`, extension included, for the editor to show. |
| `--stdin` | With `check`, read the file's text from standard input, such as an editor's unsaved buffer. The file's path is still used to resolve `use` imports and a recipe's `pipeline` line. |
| `--hovers` | With `check --json`, include what an editor shows on hover: for a pipeline, compiler-backed operation and product explanations in a `hovers` array; for any file, SPIT's own words in `words` and `word_docs`. Reads no dataset. |

Pass `-` in place of the `.spitout` to read it from standard input.

Editor hovers include signatures, inferred product types and dimensions, call-local generic bindings, producer/consumer relationships, commands, stages, and effective path templates with their provenance. Each hover names its `kind` and `name`, gives a plain-text `signature` and `details` array, and uses the same 1-based UTF-16 `line`, `column`, and exclusive `end_column` convention as diagnostics. A call to an [operation carried out by steps](docs/language-reference.md#operations-carried-out-by-steps) is described as written: its operation lists the body's steps and the steps this call expands to, each argument names the input it supplies, and a product the call makes names the call and the step that writes it. Broken lines are recovered so unrelated symbols remain available; steps that fail checking do not claim specialised types. A recipe or `.spitout` has no `hovers` array, since its names are its pipeline's. `--hovers` requires `--json`.

SPIT's own words, such as `source`, `@ vary(...)`, `where` in a `require` rule, `{@entities}`, `{image.stem}` or a `.spitout`'s `sources:`, are listed in `words`, each with its `line`, `column`, `end_column` and the `word` it is. `word_docs` gives each word used once, by its name: its `kind` (`keyword`, `selector`, `placeholder` or `header`), a SPIT `example`, a `summary` in plain text with code in backticks, and a `reference` link to its section of the [language reference](docs/language-reference.md). A word is told by where it is written, so a product called `each` is not the selector, and nothing in a comment is a word:

```json
"words": [{"line": 1, "column": 28, "end_column": 32, "word": "vary"}],
"word_docs": {"vary": {"kind": "selector", "example": "average = mean(processed @ vary(run))", "summary": "Collects a `many` input over the named dimensions, ...", "reference": "https://github.com/eclnz/spit/blob/main/docs/language-reference.md#operations-and-commands"}}
```

### Errors and warnings

Every command first reports all the problems it can find, one per line, before doing any work. Each names the line and the column where the text at fault starts:

```text
warning: line 2, column 8: source product `spare` is never used as an input
error: line 5, column 29: command for `clean` uses unknown placeholder `{result}`
error: line 9, column 14: unknown product `rwa`
```

Syntax errors are reported throughout the file first; the remaining checks run once every line parses. A step or rule that uses a declaration which failed is not reported again. Errors stop the command; warnings do not. Warnings flag a source product no step uses, an operation no step uses, a used operation with no `command` once the pipeline has commands, a product named after the operation that makes it, an operation declared in a stage but called outside it, an output type variable that no input binds, a stage with no steps, and a `#` that ends a word, which reads like a comment but is part of the word. With inputs, as in `dag`, they also flag a source with no artifacts, naming the steps it leaves without jobs, any other step that resolves no jobs, paths that differ only in letter case, and a value with a `-` that `{@labels}` writes, which BIDS cannot read back. `check` on a recipe warns when its `root` folder is missing, and `inputs` warns when it finds only some of the sources declared beside a source. A file with no steps is treated as a library of definitions, and imported definitions are never reported as unused. Jobs are resolved only when nothing else is wrong.

## Write a pipeline

Here is the complete [text processing example](examples/commands/command_demo/command_demo.spit):

```text
source shard : Lines [group, part]

path: {@product}/{@entities}.txt
path shard: input/{group}/{part}.txt

operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)

operation merge(items: many Lines) -> Lines
command merge: sort -m -u -o {@output} {items}
merged = merge(sorted @ vary(part))
```

`source` declares a family of input artifacts. A `shard` is identified by its `group` and `part` values. `sorted` keeps those dimensions. `merge` collects all parts of each group and produces one `merged[group=...]` artifact per group. The call's `@ vary(part)` names the dimension it collects.

Each step names what it makes, and the operation names what it does: `sorted = sort_lines(shard)`, not `sort_lines = sort_lines(shard)`. Later steps then read `merge(sorted ...)`, and the same operation can make two products, as `merge` does below. A product named after its operation is allowed, but `spit check` warns:

```text
warning: line 10, column 1: product `sort_lines` has the name of the operation that makes it; name the result instead, so the step reads as what it makes
```

To collect over two dimensions, collect in two steps, one dimension each: after `merged` above, `everything = merge(merged @ vary(group))` collects the per-group results into one artifact. A check that must pass before a job runs is a `verify` line beside its `command`, such as `verify merge: check_lines {items}`; see [the reference](docs/language-reference.md#operations-and-commands). A test of one file, such as that an output is not empty, is a [`check`](docs/language-reference.md#checks) declared once and attached to ports: `check nonempty: test -s {@path}`, then `-> Lines @ check(nonempty)`. A `check: nonempty` line, in a file or a stage, runs it on every output there; see [default checks](docs/language-reference.md#default-checks).

`path` lines say where artifacts live; an output with no rule goes to `out/{@product}/{@entities}`. `command` lines give the exact executable and argument order: a named input or output uses its port name, such as `{image}`, and the single unnamed output uses SPIT's `{@output}`. SPIT decides which artifacts belong to each job before filling their paths into a command.

A pipeline names no dataset. Rules about what a dataset must hold, and records of what it does hold, go in the files of step 2: `spit check` rejects a `require` rule or a `sources:` record written in a `.spit`.

### Join, collect, and check the plan

Most pipelines join a few sources and collect their results in levels. This one calibrates daily readings with the approved calibration revision, compares each with its station's baseline, which is filed under the date it was recorded rather than a reading's day, and collects the results per station and then across stations:

```text
source reading [station, day]
source calibration [station, revision]
source baseline [station, recorded]

path reading: readings/{station}/{day}.csv
path calibration: calibration/{station}-r{revision}.csv
path baseline: baseline/{station}-{recorded}.csv
path: {@product}/{@entities}.csv

operation calibrate(series, table) -> Series
command calibrate: calibrate {series} {table} {@output}
operation compare(series, reference) -> Series
command compare: compare {series} {reference} {@output}
operation summarise(series: many) -> Report
command summarise: summarise {series} {@output}

calibrated = calibrate(reading, calibration @ where(revision=3))
anomaly = compare(calibrated, baseline @ same(station))
station_report = summarise(anomaly @ vary(day))
fleet_report = summarise(station_report @ vary(station))
```

- `reading` has the most dimensions, so it drives `calibrate`: one job per reading.
- `@ where(revision=3)` keeps that revision and takes `revision` out of matching, so each reading gets its station's revision 3. The other revisions stay unused, and `dag` names them in a note.
- `@ same(station)` matches the baseline on `station` alone, whatever its `recorded` date. Each station must have exactly one.
- A port is `one` unless it says `many`: write `series`, or `series: Series` with a type, and `series: many` for a collection. There is no `one` keyword.
- `@ vary(day)` collects each station's days into one `station_report[station=...]`, in natural order, so `2026-09-02` comes before `2026-09-10`. `@ vary(station)` then collects those into one `fleet_report`, which has no dimensions, so its path is `fleet_report/global.csv`. Each level of a rollup is its own step.
- `@ each(model)` does the reverse of `vary`: it runs a step once per value of a dimension the driving input lacks. The [ragged sweep walkthrough](docs/examples.md#ragged-sweep-correlated-seeds-and-collection-order) crosses every model with the config and seed pairs a dataset holds, then collects them in two levels.

Before saving a plan, check it in three commands:

```sh
spit check stations.spit --path-rules        # the pipeline compiles; where each product goes
spit dag stations.spit --root data --counts --commands   # jobs per step, then every command line
spit dag stations.spit --root data -o stations.spitdag   # save it
```

With two stations and two days, `--counts` prints:

```text
jobs  step
   4  calibrated = calibrate
   4  anomaly = compare
   2  station_report = summarise
   1  fleet_report = summarise
  11  total
```

A count far from what the data should give, such as a step multiplied by an extra dimension, or a step with `0`, shows a mistake before anything runs. The commands show each `many` input in the order the tool receives it.

## Supply the inputs

A `.spitin` recipe describes how to find a dataset's inputs. Its first line names the pipeline it serves, relative to the recipe's folder, and its `root` line names the dataset folder. The smallest recipe is those two lines:

```text
pipeline analysis.spit
root data
```

`spit inputs` then finds every source by its path rule: each file under the root whose path matches a source's rule becomes one of that source's artifacts, with its dimensions read from the path. When that is all a dataset needs, the recipe can be left out: `spit inputs analysis.spit --root data` does the same.

Add rules when they say more than the files do. For example, beside an `analysis.spit` that declares `source image: Image [sub, ses]`, `cohort.spitin` might read:

```text
pipeline analysis.spit
root .

discover sessions: [sub, ses] from dirs data/sub-{sub}/ses-{ses}
exclude [sub] where sessions count<2
path image: data/sub-{sub}/ses-{ses}/image.nii.gz
```

`discover` reads the observed subject and session pairs from folders, and the image source expands over them, so a session folder without its image is an error rather than a session that silently has none. `exclude [sub] where sessions count<2` removes each subject with fewer than two sessions, reporting each on stderr. `require` instead fails such a group: `require [sub] where sessions count>=2` names the subject with one session and its count. `sessions` remains a discovery rule name, not a product. `exclude bold[sub=02,ses=01,run=3]  # corrupted` removes one named artifact, and `exclude from qc/excluded.csv` reads such rules from a spreadsheet; the `.spitout` records what each rule removed and why. Logical source types and operations stay in the `.spit`.

`spit check cohort.spitin` checks the rules against the pipeline without reading the dataset. `spit inputs cohort.spitin -o cohort.spitout` scans the folder its `root` line names, and writes what it found:

```text
root .

source_paths:
    image: data/sub-{sub}/ses-{ses}/image.nii.gz

contexts sessions:
    [sub=01,ses=01]:
        image

    [sub=01,ses=02]:
        image
```

The `.spitout` lists the source identities found in the dataset, and the folder it found them in, relative to the `.spitout` itself. Paths come from the pipeline's source rules; when a recipe defines a source rule instead, the `.spitout` carries that rule once in `source_paths:`. Later steps need neither the recipe nor a rescan. A dataset indexer or person can write the same inventory. A record names no file of its own: its source's path rule gives it. The text processing example uses [command_demo.spitout](examples/commands/command_demo/command_demo.spitout):

```text
sources:
    shard[group=alpha,part=01]
    shard[group=alpha,part=02]
    shard[group=beta,part=01]
```

This creates two sort jobs for `alpha`, one for `beta`, and one merge job for each group. Add another shard and SPIT creates the corresponding job without changing the pipeline.

### Start from the files

When the data already exists, `spit inputs --suggest` writes the source path rules from it. It groups the files no rule matches by shape, leaving out files at the pipeline's own output paths, makes the parts that differ between a group's files its dimensions, and prints lines to paste in. A dimension takes its name from the key before it, as `sub` in `sub-01` or `wave` in `wave3`. A key with a number, as `ses-1`, stays a dimension even when every file has the same one, and a key with a word, as `task-rest`, stays as text. Given a pipeline whose `bold` source has no rule yet:

```sh
spit inputs analysis.spit --root data --suggest
```

```text
# 3 files, such as sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz
# sub: 01, 02; ses: 1; run: 1, 2
# for `bold`, which the pipeline declares
path bold: sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_task-rest_run-{run}_bold.nii.gz

# 4 files, such as sub-01/ses-1/anat/sub-01_ses-1_T1w.json
# sub: 01, 02; ses: 1
source t1w_json .json [sub, ses]
source t1w_nii_gz .nii.gz beside t1w_json
path t1w_json: sub-{sub}/ses-{ses}/anat/sub-{sub}_ses-{ses}_T1w.json

# 1 file like no other, each a source with no dimensions if a step reads it:
#   participants.tsv
```

Each group's second line gives the values each dimension holds, the first two and the last when there are more than five, so a value that does not belong shows before the rule is pasted. A group that a source the pipeline declares without a rule fits, by its name or its dimensions, gets only its `path` line. Files that share a stem and differ by extension, as an image and its JSON, become a source and a companion declared [`beside`](docs/language-reference.md#sidecar-files) it. A dimension no word names takes the word before it and a `_`, as `field` in `field_001`; else it is called `date` when every value is a date such as `2024-01-15`, `year` when every value is a year, and `dim1`, `dim2` and so on otherwise, with a note to rename it. A dimension whose values are all dates or all years is written with that [shape](docs/language-reference.md#shapes-on-a-source-placeholder), as `{date:date}`, so a stray file such as `notes.log` is not read as a date. A plain top folder, as `baseline/` beside `raw/`, keeps its files apart, while `site_north/` and `site_south/` are one folder with a `site` dimension. A group of the same suffix as a larger one is named for a word of its own, as `bold_nback` beside `bold`.

Real folders have strays, and a suggestion does not bend its rule to fit them. When nearly every file of a group shares its keys, as `Subject01/Visit1/` and `Subject02/Visit2/` beside one `subject04/visit1/`, the stray files are left out of the rule rather than turning every word into a `dim`. A file that a suggested rule nearly matches, such as a `.bak` copy, a `_repeat` scan or one with an extra `acq-` entity, is listed with the rule and where the two part:

```text
# 2 files a rule above nearly matches but will not read; rename them, or give them a rule of their own:
#   Subject01/Visit1/T1_2024-01-15.nii.bak
#     `t1`: after `Subject01/Visit1/T1_2024-01-15.nii`, the file has `.bak` where the rule ends
#   Subject10/Visit1/T1_2024-02-11_repeat.nii
#     `t1`: after `Subject10/Visit1/T1_2024-02-11`, the file has `_repeat.nii` where the rule has `.nii`
```

Files that share their shape with no other, such as `participants.tsv` or `README`, are listed last; a rule made only of dimensions, such as `{dim1}`, would read every file and folder beside them, so none is suggested. Given a recipe, `spit inputs cohort.spitin --suggest` prints the `path` lines for the recipe, and the `source` lines, as comments, for the pipeline the recipe names. Every rule is checked against its files before it is printed, and nothing is written: read the lines, rename what needs it, and paste them in.

## Where files live

Every path SPIT reads or writes, for a source or an output, is relative to one folder: the dataset root. The root is:

- **with a recipe:** the folder its `root` line names. Every recipe has one, `root .` for its own folder, so a recipe always says where its data is, whoever runs it and from wherever;
- **with a `.spitout`:** the root its `root` line records. A printed or hand-written `.spitout` may have none; then `dag` does not check that source files exist, and the `.spitdag` records no root;
- **with a pipeline alone:** the folder `--root` gives, relative to where `spit` runs.

The root is the folder a scan reads, the base of every path, and where each source file must exist. A recipe's or `.spitout`'s root is fixed by its file; `--root` cannot change it. A recipe's `pipeline` and `root` lines are relative to the recipe's own folder and may use `..`. `spit inputs -o` records the root in the `.spitout` it writes, relative to that file, so `dag` on the `.spitout` finds the same files. A printed `.spitout` records none, since where it will be kept is unknown.

Three layouts work well:

- **Everything together:** the pipeline and recipe sit in the dataset folder, the recipe says `root .`, and `spit dag data/cohort.spitin` runs from anywhere. The scan ignores SPIT's own files, so they are not reported as matching no source rule.
- **The data in a folder of its own:** the pipeline and recipe sit together, and the recipe says where the data is:

  ```text
  pipeline cohort.spit
  root data
  ```

  `spit dag cohort.spitin` then reads `data/`, and every path rule is written relative to it.
- **The pipeline apart:** the pipeline lives with your code, and the recipe sits in the dataset folder with `pipeline ../code/analysis.spit`.

Write each path rule relative to the root: `path image: sub-{sub}/image.nii.gz` for files at `<root>/sub-01/image.nii.gz`.

## Resolve jobs

```sh
cargo run -- check examples/commands/command_demo/command_demo.spit --path-rules
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout --paths
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout --commands
cargo run -- dag examples/commands/command_demo/command_demo.spit examples/commands/command_demo/command_demo.spitout -o command_demo.spitdag
```

`dag` shows the exact command lines each job will run, `dag --paths` each artifact's file, `dag --jobs` each job's artifacts by identity, and `-o` writes the `.spitdag`. It holds everything a backend needs to run the jobs, so a backend reads nothing else: no pipeline, path rule or command template. Paths in it are relative to the dataset folder. The [`.spitdag` reference](docs/spitdag.md) describes each field.

### Find incomplete artifacts

`dag` stops at the first job the inputs cannot complete. `artifacts` resolves every job it can and reports the rest:

```sh
cargo run -- artifacts pipeline.spit inputs.spitout
```

```text
Complete artifacts: 10
  scan[subject=01,run=1]  (source)
  ...
  merged[subject=01] : Scan  (job 6: merge)

Incomplete artifacts: 2
  aligned[subject=02,run=1] : Scan  (align)
    - no `calibration` artifact for input `reference` of `align` at [run=1,subject=02]
  merged[subject=02] : Scan  (merge)
    - input `runs` needs aligned[subject=02,run=1], which cannot be produced
```

`spit artifacts dataset.spitin --by-target` answers "why can't this final report be made" without repeating a reason for every artifact it holds back. A final target is an incomplete artifact that no other incomplete job needs; each is printed once, with the incomplete artifacts it waits on nested under it and their reasons beneath. An artifact that two branches, or two targets, both wait on is written under the first only; each later job that waits on it has a line naming it and saying where it is, ``- input `a` needs a[k=1], shown above`` under the same target and ``- input `gg` needs gg, shown under uu[k=1]`` under an earlier one. A chain more than 20 levels deep stays at the 20th indent, so a long chain costs a line for each step and not a wider indent for each; each line is still the artifact the line above it waits on. For the example above:

```text
Complete artifacts: 10

Final targets that cannot be made: 1 (incomplete artifacts: 2)
  merged[subject=02] : Scan  (merge)
    aligned[subject=02,run=1] : Scan  (align)
      - no `calibration` artifact for input `reference` of `align` at [run=1,subject=02]
```

An incomplete artifact has a missing or ambiguous input, a collection below its `@ min(count)`, or an input that is itself incomplete, so a gap early in the pipeline is traced through every step that depends on it. Given a recipe, a group that fails a `require` rule is listed under `Coverage gaps`, and its sources are held back from every job. A step creates jobs only for the artifacts that drive it, so a context with no driving artifact at all appears only through the coverage gaps and steps that notice it missing. The command succeeds whatever it finds; the complete artifacts are the ones the pipeline could produce from these inputs today.

`artifacts` also lists, under `Unused sources`, each source that no job reads, whether or not that job can be completed. Some are left out on purpose, such as calibration revisions a `where(revision=3)` selector passes over; others point to a mistake, such as `pricing/S07.json` read as store `S07` where the pipeline needs `s07`. `dag` names them in a note when there are at most three, as `2 source artifacts are used by no job: calibration[station=north,revision=1], calibration[station=north,revision=2]`, and otherwise counts them by product, as `5 source artifacts are used by no job (calibration: 5)`.

While it settles a `.spitin` recipe's inputs, in `inputs`, `dag` and `artifacts`, SPIT notes each dimension whose values differ only in ASCII letter case, as ``note: `store` has values that differ only in ASCII letter case, which are different values to SPIT: `S07` in pricing, `s07` in sales``, naming the sources that have each spelling and `(excluded)` for one an `exclude` rule removed, so a group exclusion that missed a source filed under another spelling is seen at once. Only ASCII letters fold, so `é` and `É` are different values with no note, and a `.spitout` given directly to `dag` or `artifacts` is not settled, so it gets no note. When a missing input differs from an unused source only in letter case or leading zeros, the failed `dag` and `artifacts` reports name that source and the differing dimension. They also warn that the source is unused. A genuinely missing source has no such hint. During discovery, `inputs` notes how many files match no source rule, naming them when there are at most three and otherwise counting them by extension, as ``24 files under `.` match no source rule and are not read (23 `.json`, 1 `.tsv`), such as `dataset_description.json` ``; files at the pipeline's own output paths, such as an earlier run's, are not counted. A source whose path rule matches no file at all gets a warning that names the unmatched file nearest the rule, and shows where the two part:

```text
warning: source `bold` matched no files with path rule `data/sub-{sub}/ses-{ses}/func/sub-{sub}_ses-{ses}_run-{run}_bold.nii.gz`
  the nearest file is `data/sub-01/ses-1/func/sub-01_ses-1_task-rest_run-1_bold.nii.gz`
  after `data/sub-01/ses-1/func/sub-01_ses-1_`, the file has `task-rest_run-1_bold.nii.gz` where the rule has `run-{run}_bold.nii.gz`
```

The nearest file is the one the rule matches furthest from its start, and a file is named only when the rule matches some of its start or it ends as the rule does. The warning comes from every scan, with a recipe or a pipeline and `--root`, so `inputs`, `dag` and `artifacts` all give it. If a `require` count fails because the scan found no files for its source, `inputs` and `dag` also name the path rule they used, and when no file comes near the rule, an unmatched file containing the source name. Run `spit inputs dataset.spitin --unmatched` to list unmatched files, even when a `require` rule fails.

To make a plan for the work that can run now, use `spit dag dataset.spitin --partial -o plan.spitdag`. The command succeeds and records each unproducible output and its reasons in `left_out`. A `many` input takes only its complete members, then applies any `@ min(count)` requirement to that smaller collection. For example, a chain summary can use the reports from good stores even when other stores' reports cannot be made. Use `spit artifacts dataset.spitin` to inspect the full set of gaps before running the plan.

## Language reference

Beyond the basics above, `.spit` files support typed products, multi-output operations, `many`/aggregation inputs with selectors (`where`, `same`, `vary`, `each`), symbolic type variables, stages, path placeholders, and `use` imports for sharing definitions across files; `.spitin` recipes add a dataset `root`, directory discovery, and named or conditional `exclude` rules, plus `require` checks.

Paths are written once where they can be. SPIT's own path placeholders take `@`, as `{@product}`, `{@entities}`, `{@stage}` and `{@labels}`, so a bare `{sub}` is always a dimension. An operation names the extension each output's file has, as `-> Transform .mat`, and `ext:` sets one for the rest, so a default path is written without one. A recipe's `path:` is the default for sources, so where a dataset keeps its inputs stays with the dataset, and the pipeline's default can place outputs by stage. A source declares its files' extension as an operation declares its outputs', `source events .tsv [sub]`, so one default covers sources of different formats. A file that shares another file's stem is declared `beside` it. For sources, the companion inherits the main source's dimensions and path; for outputs, the same job writes both files. A recipe can give the main source's path for one dataset. A command can take an output's folder and name, `{image.dir}` and `{image.stem}`, for a tool that adds the extension itself. A `/` after a type makes a product's artifacts folders, as `source dicom : Dicom / [sub]` or `-> (subject: FsSubject /)`, for a tool that reads or writes a folder of files. A path can use `{@labels}` for BIDS-style dimension labels and `[...]` for a segment only some products have; the [cohort walkthrough](docs/examples.md#cohort-discovery-exclusion-and-grouped-removal) shows one default path for run, session, and subject outputs. See the [full language reference](docs/language-reference.md) for syntax and rules for each of these.

## More examples

| Example | Shows |
| --- | --- |
| [Basic](examples/basic/basic.spit) | A small pipeline, with its recipe and inputs in separate files |
| [Untyped](examples/types/untyped.spit) | Resolution without types |
| [Typed](examples/types/typed.spit) | Parameterized symbolic types |
| [Branching](examples/pipelines/branching.spit) | Shared inputs and branches |
| [Complex](examples/pipelines/complex.spit) | Nested aggregation |
| [Selectors](examples/pipelines/selectors.spit) | `where`, `same`, a two-output step, a verification, and a many input beside a single input |
| [Analytics](examples/analytics/analytics.spit) | Joins and rollups |
| [Cohort](examples/patterns/cohort/cohort.spit) | BIDS sessions with discovery, a dropped subject, an excluded run, and one default path using `[...]` groups and `{@labels}` |
| [Field survey](examples/commands/field_survey/field_survey.spit) | A larger pipeline with sidecar files, calibration, alignment between spaces, and commands |
| [MRtrix3 ACT](examples/commands/mrtrix3_act/mrtrix3_act.spit) | A larger pipeline with commands in nested preprocessing, anatomy, and tractography stages, with a folder per stage, per-stage file formats through `ext:`, and DWI sidecars |
| [Stages](examples/stages/stages.spit) | Preprocessing and analysis stages, a stage's own path default, and `{@stage}` paths |
| [Nested stages](examples/stages/nested.spit) | Stages within a stage, beside a step in the outer stage itself |
| [Imports](examples/imports/imported.spit) | Reuse source and operation definitions with `text::` names |
| [Diffusion preprocessing by steps](examples/composites/mrtrix/act.spit) | The ACT example's preprocessing and registration as two calls to operations a library carries out by steps |
| [Variant calling by steps](examples/composites/germline/somatic.spit) | One operation carried out by steps, called for a tumour and its normal, with checks that add up |
| [Compiler stress pipelines](examples/stress/README.md) | Deep type inference, deliberate type errors, uneven joins, and large multilevel DAGs |

Run `cargo test --test source_files` to see the field survey example checked against a temporary tree of empty source files: it resolves when every file is present, and reports a missing file, a photo without its sidecar, and a source path that is a directory. The MRtrix example imports each DWI's `.bvec`, `.bval`, and JSON metadata into a `.mif` before processing.

## How SPIT works

```text
.spit ──► 1. check ──► compiled pipeline
                            │
.spitin + data ──► 2. inputs ──► .spitout
                            │        │
                            ▼        ▼
                        3. dag ──► .spitdag
```

The pipeline supplies operations and rules; the `.spitout` supplies artifact identities and their files. Resolution checks dimensions, matching, cardinality, and any known types, then binds each artifact to its file and expands each command into its arguments. Step 2 and step 3 each build on step 1 and never on each other, and a backend reads only the `.spitdag`. SPIT does not inspect file contents or command-specific metadata itself; `verify` commands and `check`s run those checks with your own tools.

## Documentation

- [Documentation home](docs/index.md) — guided chapters for learning the language and inspecting plans
- [Getting started](docs/getting-started.md) — build SPIT and inspect a complete example
- [Language catalog](docs/reference/index.md) — browse keywords, operation forms, selectors, and placeholders
- [Language reference](docs/language-reference.md) — full `.spit`, `.spitin` and `.spitout` syntax
- [Command line reference](docs/cli.md) — command forms and inspection flags
- [The `.spitdag` format](docs/spitdag.md) — every field a backend reads
- [Architecture](docs/architecture.md) — the internal model: resolution, typing, and the bound DAG
- [Examples](docs/examples.md) — complete walkthroughs for sweeps, cohorts, selectors, and stages, plus the larger pipeline catalog

To browse and search these pages locally, run `python3 -m pip install -r .github/docs-requirements.txt` and `mkdocs serve` from the repository root. `mkdocs build --strict` checks the documentation site before publishing it.

## Development

```sh
cargo test
```

Runs the full test suite, including the integration tests under `tests/` that check the example pipelines end to end.

To time the CLI on large generated pipelines and datasets, or profile it, see [profiling](profiling/README.md).

## Contributing

Issues and pull requests are welcome. For a change to the language or resolver, add or update a test under `tests/` and, if it changes behavior described here, update this README or the [language reference](docs/language-reference.md) alongside it.

## Disclaimer

SPIT was developed with the help of generative AI tools. I am not a Rust developer: most of the code and documentation was generated by AI, and I have checked it by testing its behavior rather than by expert review of the Rust itself. The test suite covers the examples and language features, but the code may still contain errors or unidiomatic Rust. The commands in a `.spitdag` run on your system when a backend runs them: review them before running them, especially on data you cannot easily replace. The software is provided as is, without warranty of any kind; see the [license](LICENSE).

## License

SPIT is released under the [MIT License](LICENSE).
