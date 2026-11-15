# Glossary and cheat sheet

The terms the guide uses, and the syntax and commands to keep beside you. Each entry points to the page that explains it.

## Glossary

| Term | Meaning |
| --- | --- |
| **Artifact** | One file or folder that a pipeline reads or makes. See [Artifacts](artifacts.md). |
| **Product** | A named family of artifacts, such as `reading` or `cleaned`. |
| **Dimension** | A label that tells the artifacts of a product apart, such as `region` or `site`. A product's dimensions are listed in brackets: `reading [region, site, day]`. |
| **Identity** | A product with its dimension values: `reading[region=coast,site=dune,day=2025-06-26]`. |
| **Source** | A product the dataset supplies. A source declares its type, extension, dimensions and checks, and has a path rule. |
| **Operation** | A reusable piece of work with named inputs and outputs and a command, or a body of steps. See [Operations](operations.md). |
| **Port** | One named input or output of an operation. Its name is its placeholder in the command. |
| **Step** | A call of an operation on products, such as `cleaned = clean(reading @vary(day), calibration)`. See [Pipelines](pipelines.md). |
| **Job** | One run of an operation on one set of artifacts. A step makes one job for each artifact that drives it. |
| **Stage** | A named group of steps that shares defaults, such as a path. A stage does not order the jobs. |
| **Pipeline** | A `.spit` file: sources, operations and steps, for any dataset. |
| **Recipe** | A `.spitin` file that points a pipeline at one dataset, and says what to exclude, require and review. See [Datasets](datasets.md). |
| **Inventory** | A `.spitout` file: the settled source records of a dataset. |
| **Root** | The dataset's folder, to which source paths are relative. |
| **Output root** | The folder that generated files and commands are relative to. It is the dataset root unless the recipe or pipeline names another. |
| **Context** | A group that SPIT has seen as a folder on disk, whether or not it holds files. |
| **Directory view** | A folder that other jobs' files are written into, read as one input. It has no command of its own. See [directory views](artifacts.md#directory-views). |
| **Collection** | The group of artifacts that a `many` port takes, chosen by `@vary(...)`. |
| **Selector** | A clause that narrows or changes how an input matches: `@where`, `@same`, `@vary`, `@each`. |
| **Check** | A test of one artifact, declared once and attached with `@check(...)`. |
| **Verify** | A test of a job's inputs, run before its command. |
| **Review** | A person's judgement of an artifact, recorded in a file the recipe names. |
| **Tool** | A program whose version a recipe records in a `.spitenv`, so that a runner reruns jobs when it changes. |
| **Plan** | The jobs SPIT resolves for a pipeline and a dataset. Saved as a `.spitdag`. See [Plans](plans.md). |
| **Runner** | A program that carries out a plan. `spit-bash` runs on one machine. See [Runs](runs.md). |
| **Record** | What the runner keeps about a finished job, so that it can tell later whether the job is current. |

## Files

| File | Holds |
| --- | --- |
| `.spit` | A pipeline |
| `.spitin` | A recipe |
| `.spitout` | An inventory of a dataset's inputs |
| `.spitenv` | The identities of the tools on a machine |
| `.spitdag` | A plan |

## Pipeline statements

| Statement | Example |
| --- | --- |
| Source | `source reading : Readings<Raw> .csv [region, site, day] @check(day_of_hours)` |
| Dimension order | `dimensions [region, site, day, alert]` |
| Type | `type Frame<Space>` |
| Operation | `operation clean(days: many Readings<Raw> @min(2), calibration: Calibration) -> Readings<Clean>` |
| Command | `command clean: python3 clean.py {days} -o {@output}` |
| Step | `cleaned = clean(reading @vary(day), calibration)` |
| Path rule | `path reading: {region}/{site}/{day}.csv` |
| Defaults | `default path:`, `default ext:`, `default entities:`, `default check:`, `default review:`, `default uses:` |
| Check | `check nonempty: test -s {@path}` |
| Verify | `verify register: check_same_grid {moving} {reference}` |
| Review | `review looked_at`, attached with `@review(looked_at)` |
| Resources | `resources align: cores=4, memory=8G` |
| Tool | `tool fsl`, with `uses register: fsl` |
| Anchored | `anchored feat` |
| Folder output | `-> Bundle /`, started as a copy of an input with `-> Catalogue / from bundle` |
| Directory view | `-> Pack / view`, whose inputs are the files it holds |
| Stage | `stage clean:` and indented steps |
| Import | `use figure from figure.spit as fig` |
| Root, output | `root data`, `output results` |
| Strict types | `types strict` |

## Recipe statements

| Statement | Example |
| --- | --- |
| Pipeline | `pipeline weather.spit` |
| Root, output | `root data/raw`, `output data/results` |
| Source paths | `inputs:` with `reading: {region}/{site}/{day}.csv` |
| Directory contexts | `contexts: sub-{sub}/ses-{ses}` inside `inputs:` |
| Exclude a name | `exclude reading[region=valley,site=sedge,day=2025-04-15]  # reason` |
| Exclude a group | `exclude [site=sedge]` |
| Exclude by condition | `exclude [sub] where t1w count<2` |
| Require | `require [region, site] where calibration count=1` |
| Review file | `review looked_at from qc/looked_at.csv` |
| Tools | `tools from runtime.spitenv` |

## Clauses on an input

| Clause | Effect |
| --- | --- |
| `@vary(day)` | Gather every `day` into one collection, and drop `day` from the output |
| `@each(alert)` | Run once for every value of `alert`, and add it to the output |
| `@where(site=dune)` | Keep artifacts with that value, and take the dimension out of matching |
| `@same(region)` | Match on `region` alone |
| `@min(2)` | On a `many` port: refuse a group with fewer than two |
| `@check(nonempty)` | Test the artifact |
| `@review(looked_at)` | Hold the artifact until it has a `pass` |

## Placeholders

| In | Placeholder | Value |
| --- | --- | --- |
| Command | `{series}` | A port's path |
| Command | `{days}` | A `many` port: one argument for each artifact |
| Command | `{@output}` | The path of the one unnamed output |
| Command | `{low}` | The path of the output named `low` |
| Command | `{low.dir}`, `{low.stem}` | An output's folder, and its name without the extension |
| Command | `{alert}` | The job's value of a dimension |
| Command | `{days.day}` | A `many` port's value of a dimension, one for each artifact |
| Command | `@join(days)`, `@list(days)` | A `many` port as one argument, or as a file of paths |
| Command | `@scalar(size, min=1)` | The number in a port's file |
| Command | `{@cores}` | The cores the job was given |
| Command | `{@here}` | The folder of the file that declares the operation |
| Command | `<`, `>` | Standard input and output, with one placeholder |
| Path | `{region}` | A dimension's value |
| Path | `{@product}`, `{@entities}`, `{@stage}` | The product's name, its dimensions, and its stage |
| Path | `[...]` | A part that appears only when its placeholders have values |
| Check | `{@path}` | The artifact being tested |

## Commands

| Command | Does |
| --- | --- |
| `spit check FILE` | Check a pipeline, recipe, inventory or tools file. `--path-rules`, `--calls`, `--json`, `--stdin`, `--hovers` |
| `spit inputs RECIPE` | Scan the dataset and print the inventory. `-o FILE`, `--unmatched` |
| `spit dag RECIPE` | Plan the jobs. `--counts`, `--commands`, `--jobs`, `--paths`, `--tree`, `--partial`, `--json`, `-o FILE` |
| `spit artifacts RECIPE` | Explain what can and cannot be made. `--by-target` |
| `spit-bash plan FILE` | Say what a run would do. `--verbose`, `--json` |
| `spit-bash explain ID FILE` | Say why one job would run or be skipped |
| `spit-bash run FILE` | Run the jobs. `-j`, `--memory`, `--partial`, `--force`, `--compare`, `--move-files` |
| `spit-bash log FILE` | List the logs of chosen jobs |
| `spit-bash adopt FILE` | Record jobs whose files already exist as current |
| `spit-bash accept-tools FILE` | Accept the tools as they are for finished jobs |
| `spit-bash restore FILE --run RUN` | Put back what a run replaced |
| `spit-bash prune FILE --keep-runs N` | Remove old run folders |
| `spit-bash clean FILE FOLDER` | List, or with `--yes` delete, files the plan does not explain |

Every `spit-bash` command takes `--entity`, `--product` and `--stage` to choose part of the plan, though `clean` refuses a selection, since the jobs left out would make their outputs look like strays.
