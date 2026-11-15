# Operations

An **operation** is a reusable piece of work: it says what a job reads, what it makes, and which command makes it. A [step](pipelines.md#steps) applies an operation to products, and each job SPIT plans is one run of an operation on one set of artifacts. This page covers how operations are declared, how their commands are written, and how to declare what a job needs and how to test it. The examples come from the [showcase analyses](https://github.com/eclnz/spit-showcase), such as the [weather network](walkthrough.md).

## Signatures

A signature lists the inputs a job takes and the outputs it makes. The weather network's cleaning step reads many day files and one calibration, and makes one series:

```spit
operation clean(days: many Readings<Raw> @min(2), calibration: Calibration) -> Readings<Clean>
```

- Every input is a **port** with a name, and the name is its placeholder in the command. A port is written `name`, `name: Type`, `name: many` or `name: many Type`.
- `calibration` takes one artifact for each job. `days` takes a **collection**, so its port says `many`. Which dimensions the collection gathers is decided by the step, with `@vary(...)`.
- A call's arguments follow the port order, and SPIT checks each argument's type against its port.
- A port can carry a bound on its collection. `@min(2)` rejects a site with fewer than two days.

An operation that makes several outputs names each one. The step assigns one product to each, in order, and the command refers to the files by their names. The classifier analysis splits each dataset once into training and test sets:

<!-- spit-doc: setup
source dataset : Table .csv [dataset]
source split_py : Python .py
default entities: {@key}={@value} separated "_"
default path: data/out/{@product}[/{@entities}]
-->

```spit
operation split(script: Python, table: Table) -> (train: Table .csv, test: Table .csv)
command split: python3 {script} {table} {train} {test}
train, test = split(split_py, dataset)
```

A command must use every output it makes. The one exception is an output declared [`beside`](artifacts.md#companion-outputs) another.

An operation that no step calls is not checked until a step calls it, so a file of operations to import into other pipelines needs no data of its own. An operation can also be carried out by steps instead of a command: the weather figure is one (see [composite operations](pipelines.md#composite-operations)).

In the [weather family](weather.md):

- An operation: `operation day_mean(day) -> Mean` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A port that takes a collection: `operation site_mean(means: many Mean) -> Mean` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A bound on a collection: `operation clean(days: many Readings<Raw> @min(2), offset: Offset) -> Readings<Clean> .csv` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- An operation with named outputs: `-> (mean: Mean, spread: Mean` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A step that assigns several products: `site_mean, site_spread = summarise(thinned)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

## Command templates

A `command` line gives an executable and its arguments. SPIT fills in the placeholders for each job and records the result in the plan. The weather network's `reduce` operation reads a site's series and its location:

<!-- spit-doc: setup
source cleaned : Readings<Clean> [region, site]
source location : Site .json [region, site]
default path: [{region}/][{site}/]{@product}
default ext: .csv
-->

```spit
operation reduce(series: Readings<Clean>, location: Site) -> Readings<SeaLevel>
command reduce: python3 {@here}/steps/reduce.py --location {location} -o {@output} {series}
reduced = reduce(cleaned, location)
```

For the site `valley/hazel`, `spit dag --commands` shows the command with the pipeline's own folder at `{@here}`:

```text
Job 33  reduce  [compare]
  run:    python3 /path/to/weather/steps/reduce.py --location ../raw/valley/hazel/site.json -o valley/hazel/reduced.csv valley/hazel/cleaned.csv
```

`{location}` and `{series}` are the paths of the ports' artifacts, `{@output}` is the path of the operation's one unnamed output, and a named output uses its own name, as in `{train}` above. Paths are relative to the folder the commands run from, which is the output root.

- The first word must be an executable on `PATH`, or a path to one. SPIT does not install tools or load shell functions.
- A command is a list of words, not a shell script. Quote text that has spaces, as in `--title "my plot"`. A substituted path always stays one argument, whatever characters it holds.
- Shell syntax such as `|`, `&&`, `;` and `$VARIABLE` is not expanded. To use it, run a shell yourself and pass the paths as arguments, as the weather network's `day_of_hours` check does: `sh -c 'test "$(wc -l < "$1")" -eq 25' sh {@path}`.
- A command can wrap over several indented lines, and the operation's signature can too, while its brackets are open.

In the [weather family](weather.md):

- A command: `command day_mean:` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)
- A command written over several lines: `clean: awk -F, -v site={site} -v off=@scalar(offset)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

### Placeholders

| Form | Value |
| --- | --- |
| `{series}` | The path of an input port's artifact |
| `{days}` | A `many` port: one argument for each artifact, in collection order |
| `{@output}` | The path of the single unnamed output |
| `{train}` | The path of the output named `train` |
| `{image.dir}`, `{image.stem}` | An output's folder, and its file name without the extension |
| `{alert}` | The value of the job's `alert` dimension, as text |
| `{days.day}` | A `many` port: each artifact's `day` value, one argument each |
| `@join(images)` | A `many` port's paths as one argument, separated by `,` |
| `@list(images)` | The path of a file that lists a `many` port's paths, one to a line |
| `@scalar(size, min=1)` | The number held in the `size` input's file |
| `{@cores}` | The number of cores the job was given |
| `{@here}/steps/reduce.py` | A script beside the file that declares the operation |

The sections below say more about the ones that need it.

**Job values.** A placeholder that names a dimension gives the job's value as text. The weather network's tally job names its alert, and the variant-calling analysis writes each sample's name into the read group:

<!-- spit-doc: setup
source cleaned : Readings<Clean> [region, site]
source threshold : Threshold .json [alert]
dimensions [region, site, alert]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation tally(series: Readings<Clean>, threshold: Threshold) -> Count
command tally: python3 {@here}/steps/tally.py --alert {alert} --threshold {threshold} -o {@output} {series}
alert_days = tally(cleaned, threshold @each(alert))
```

<!-- spit-doc: setup
source index : BwaIndex /
source r1 : Fastq .fastq.gz [sample]
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
operation align(index: BwaIndex, r1: Fastq) -> Sam .sam
command align: bwa mem -M -R "@RG\tID:{sample}\tSM:{sample}" -o {@output} {index}/genome {r1}
aligned = align(index, r1)
```

A job has the dimensions of its outputs, so a step that collects with `@vary(day)` has no single `{day}`. Use `{days.day}` for each collected artifact's value, one argument for each artifact in the order `{days}` gives their paths. The weather network's cleaning command uses it to hand the script each file's date. See [a job's dimension values](../language-reference.md#a-jobs-dimension-values).

**Collections as one argument.** `{days}` gives a tool one argument for each artifact. A tool that wants them in a single argument, or in a file, uses `@join(...)` or `@list(...)`:

<!-- spit-doc: output
spit dag scans.spit scans.spitout --commands
--- scans.spit
source scan : Image .nii.gz [sub, run]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation average(images: many Image) -> Image .nii.gz
command average: mean_tool --inputs=@join(images) -o {@output}
operation register(images: many Image) -> Table .tsv
command register: register_tool --file-list @list(images) -o {@output}
averaged = average(scan @vary(run))
registered = register(scan @vary(run))
--- scans.spitout
sources:
    scan[sub=01,run=2]
    scan[sub=01,run=10]
-->

```text
  run:    mean_tool --inputs=out/scan/sub=01_run=2.nii.gz,out/scan/sub=01_run=10.nii.gz -o out/averaged/sub=01.nii.gz
  run:    register_tool --file-list '@list(out/scan/sub=01_run=2.nii.gz, out/scan/sub=01_run=10.nii.gz)' -o out/registered/sub=01.tsv
```

Both forms list artifacts in collection order, so run 2 comes before run 10. The runner writes the `@list` file when the job starts and removes it when the job ends. See [a many input as one argument](../language-reference.md#a-many-input-as-one-argument).

**Numbers from files.** When a tool needs a number that another tool prints, write the number to a file in one step and read it into a later command with `@scalar`:

<!-- spit-doc: output
spit dag numbers.spit numbers.spitout --commands
--- numbers.spit = examples/runnable/numbers/numbers.spit
--- numbers.spitout
sources:
    list[plot=garden]
-->

```text
  run:    wc -l < input/garden.txt > output/size/plot=garden.txt
  run:    awk -v 'n=@scalar(output/size/plot=garden.txt, min=1)' '{ print NR "/" n ": " $0 }' input/garden.txt > output/numbered/plot=garden.txt
```

When the second job starts, the runner reads the number in `size`'s file and runs `awk -v n=3 ...`. A file that holds anything but one number, or a number under `min`, fails the job before `awk` starts, so an empty list never reaches it. See [numbers read from files](../language-reference.md#numbers-read-from-files).

**Scripts and settings as inputs.** A job reruns when a file it reads changes, so a script or a settings file that decides the result should be an input. The terrain analysis lists its measuring script and each region's grid options as sources, and the classifier analysis does the same for each step's script:

<!-- spit-doc: setup
source dem : Dem .tif [region]
source slopes : Dem .tif [region]
default entities: {@key}={@value} separated "_"
default path: data/out/{@product}[/{@entities}]
-->

```spit
source row : Python .py
operation measure(row: Python, dem: Dem, slope: Dem) -> Table .tsv
command measure: python3 {row} {region} {dem} {slope} > {@output}
rows = measure(row, dem, slopes)
```

Editing `row.py` reruns the three region rows, and the table only if a row comes out different. A script that sits beside the pipeline is declared `here`, which SPIT finds from the pipeline's folder wherever the dataset is:

<!-- spit-doc: setup
source durations : Json [sample]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
source tabulate : Script .py here
path tabulate: scripts/table.py

operation table(durations: Json, script: Script) -> Table .tsv
command table: python3 {script} {durations} > {@output}
summary = table(durations, tabulate)
```

The weather and quality-control pipelines name their scripts with `{@here}` instead. That works from any folder, but editing such a script reruns nothing, and `spit dag` notes each one so the choice is deliberate. [Files beside the pipeline](../language-reference.md#files-beside-the-pipeline) covers both.

In the [weather family](weather.md):

- A job's own value in a command: `-v site={site}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- Each artifact's value of a dimension: `{means.site}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A collection as one argument: `@join(means)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A collection as a file of paths: `@list(means)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A number read from a file: `@scalar(offset)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A script beside the pipeline as a source: `tabulate : Script .py here` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A script found with the pipeline's folder: `command write: python3 {@here}/report.py {board} {report.dir} {report.stem}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

### Standard input and output

A tool that prints its result instead of writing a file is given `>` and an output placeholder, and one that reads standard input is given `<` and an input placeholder. The spoken-digits analysis asks `soxi` for each recording's duration:

<!-- spit-doc: setup
source clean : Wav .wav [speaker, digit, take]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation duration(audio: Wav) -> Seconds .txt
command duration: soxi -D {audio} > {@output}
seconds = duration(clean)
```

The runner opens the file and connects the tool's output to it, so no shell is involved. `<` and `>` are words of their own, followed by one placeholder, and a command has at most one of each. `tr a-z A-Z < {list} > {@output}` uses both. Only an operation's `command` can use them. A `verify` or a `check` that needs a redirect runs a shell.

In the [weather family](weather.md):

- A command that reads standard input and writes standard output: `count: wc -l < {series} > {@output}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A command that writes standard output: `> {@output}` in [v1_means.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v1_means/v1_means.spit)

### Several commands in one job

A job sometimes needs two tools in turn, as `samtools index` finishes what `samtools sort` wrote. Leave the `command` line empty after its `:` and put each command on its own indented line. They run in order and stop at the first that fails. This is the variant-calling analysis's alignment sort, and it uses `{@cores}` too:

<!-- spit-doc: setup
source sam : Sam .sam [sample]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation sort(sam: Sam) -> (bam: Bam .bam, bai: Bai +.bai beside bam)
resources sort: cores=2, memory=1G
command sort:
    samtools sort --threads {@cores} -o {bam} {sam}
    samtools index {bam}
sorted, sorted_bai = sort(sam)
```

`spit dag --commands` shows a `run:` line for each. Every placeholder works on every line, and between them the lines must use every output. See [commands of several lines](../language-reference.md#commands-of-several-lines).

In the [weather family](weather.md):

- Several commands in one job: `order:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

## Tools

A runner reruns a job when its command or its files change, but it cannot see a tool's version, so a job made with an old version of a tool still looks current after an upgrade. A recipe fixes this by naming a `.spitenv` file that records what each tool is on the machine where the jobs run. This pipeline declares its tool and the operations that use it, and the recipe names the file:

<!-- spit-doc: recipe
--- bundle.spit
source raw : Text [n]
path raw: input/{n}.txt
tool python
operation joined(xs: many Text) -> Bundle .json
uses joined: python
command joined: python3 bundle.py joined @join(xs) {@output}
joined_result = joined(raw @vary(n))
path joined_result: joined.json
--- runtime.spitenv
tools:
    python: probe python3 --version
-->

```spit
pipeline bundle.spit
root .
tools from runtime.spitenv
```

The `.spitenv` has one line for each tool, `name: identity`. An identity is either fixed text, such as a version or a container digest, or `probe` followed by a command, whose output is the identity. The runner runs each probe once, before any job, and a probe that fails stops the run.

- With no `uses` line, every job depends on every tool in the `.spitenv`. That never misses a rerun, and may cause a few that were not needed.
- `tool python` declares a tool in the pipeline, so that `spit check` can catch a misspelled name. `uses joined: python` limits `joined` jobs to that tool.
- When a tool's identity changes, the runner reruns the jobs that depend on it. To accept an upgrade that should not rerun finished work, use [`spit-bash accept-tools`](runs.md#reuse).

Some tools write their own paths into an output, so a moved file points at a place that does not exist. FSL FEAT's `design.fsf` is one. Mark such an operation with `anchored`:

<!-- spit-doc: setup
source design : Fsf .fsf
source bold : Bold .nii.gz [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
operation feat(bold: Bold, design: Fsf) -> FeatDir .feat/
command feat: feat {design} {bold} {@output}
anchored feat
firsts = feat(bold, design)
```

An anchored job reruns when the path of a file it reads or writes changes, and the runner never moves its outputs. Use it only when the outputs depend on stored paths, not for tools that merely record how an output was made. See [tools](../language-reference.md#tools) and [tools that store their paths](../language-reference.md#tools-that-store-their-paths).

In the [weather family](weather.md):

- The tools a recipe records: `tools from ../shared/network.spitenv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A tool's identity from a probe: `python: probe python3 --version` in [network.spitenv](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/network.spitenv)
- A tool's identity as fixed text: `awk: system-awk` in [network.spitenv](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/network.spitenv)
- A tool declared in the pipeline: `tool:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- The tools an operation depends on: `write: python, !awk` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- The tools every operation depends on: `uses: awk` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- An operation whose output stores its paths: `anchored:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

## Resources

A runner starts several jobs at once. A `resources` line says what each job of an operation needs, so it starts only as many as the machine can hold. The variant-calling analysis declares what its aligner needs and hands it the cores:

<!-- spit-doc: setup
source index : BwaIndex /
source r1 : Fastq .fastq.gz [sample]
default entities: {@key}={@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
operation align(index: BwaIndex, r1: Fastq) -> Sam .sam
command align: bwa mem -t {@cores} -o {@output} {index}/genome {r1}
resources align: cores=2, memory=1G
aligned = align(index, r1)
```

Each `align` job runs `bwa mem -t 2 ...`, and the runner starts one only when two cores and one GiB of memory are free. Write `{@cores}` where the tool takes its thread count, so the tool uses what the runner set aside.

- `cores=N` is a whole number from 1, and `memory=SIZE` is a whole number with a unit `K`, `M`, `G` or `T`, each 1024 times the one before. Give either or both.
- An operation without a `resources` line needs one core and claims no memory, and `{@cores}` is `1`.
- `spit-bash` stops a job that holds more memory than it declares, so declare the most the job needs on your largest input.
- A changed `resources` line does not make a finished job out of date, unless the command's `{@cores}` changes with it.

`spit dag --commands` shows a `needs:` line above the `run:` line of each job that declares resources. See [cores and memory](../language-reference.md#cores-and-memory).

In the [weather family](weather.md):

- The cores a job was given: `command league: sort -g -r --parallel={@cores} -o {@output} {means}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- The resources a job needs: `resources league: cores=2, memory=64M` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

## Checks and verification

SPIT does not run tests of its own. It writes the commands you declare into the plan, and the runner runs them. A failed test fails the job and stops the jobs that depend on it.

### Verification

`verify` tests a job's **inputs** before its main command. Use it for a condition that involves more than one input, or one the command would only discover late:

<!-- spit-doc: output
spit dag checked_copy.spit checked_copy.spitout --commands
--- checked_copy.spit = examples/runnable/verify/checked_copy.spit
--- checked_copy.spitout
sources:
    text[part=one]
-->

```text
  verify: test -s input/one.txt
  run:    cp input/one.txt output/copied/part=one.txt
```

A `verify` command may use any input port, including a `many` port, and `{@cores}`. If it fails, the main command does not run. `spit dag --commands` lists each job's `verify` lines above the command they guard.

In the [weather family](weather.md):

- A test of a job's inputs: `verify clean: test -s {offset}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

### Artifact checks

A `check` tests **one artifact**. Declare it once and attach it with `@check(...)`. The quality-control pipeline declares a check with a parameter, and writes the lab's policy where a reader sees it:

<!-- spit-doc: setup
source t1w : T1w .nii.gz [sub]
source summaries : Json .json [sub]
default entities: {@key}-{@value} separated "_"
default path: out/{@product}[/{@entities}]
-->

```spit
check usable(min_snr): python3 -c 'import json, sys; snr = json.load(open(sys.argv[1]))["snr"]; sys.exit(0 if snr is not None and snr >= float(sys.argv[2]) else f"SNR {snr} is below {sys.argv[2]}")' {@path} {min_snr}

operation approve(summary: Json) -> Json .json @check(usable(35))
command approve: cp {summary} {@output}
approved = approve(summaries)
```

`{@path}` is the path of the artifact being tested, and the check must use it. Each `{min_snr}` is the word given where the check is attached, so `usable(35)` runs the script with the summary's path and `35`. A subject whose signal-to-noise ratio is under 35 fails `approve`, and so does every job that reads `approved`, while the other subjects, and the table of every subject, go on. The digits analysis declares a check that a trimmed recording still holds audio, and attaches it to the output, `@check(audible)`. Where `@check(...)` goes decides when it runs:

- On an **output**, as above, it runs after the command, before the job counts as done.
- On an **input port** or a **source**, it runs on each artifact the port reads, before the job's `verify` commands and command. The weather network attaches `@check(day_of_hours)` to its readings.
- `default check: nonempty` applies a check to every output in the file or stage. `!nonempty` drops an inherited one.

A check reads only its own artifact and never adds a dependency between jobs. When the job that writes an artifact checks it, a job in the same plan that reads it does not run the same check again. See [checks](../language-reference.md#checks) and [default checks](../language-reference.md#default-checks).

Next: [Pipelines](pipelines.md).

In the [weather family](weather.md):

- A check on one artifact: `check rows(n): sh -c 'test "$(wc -l < "$1")" -eq "$2"' sh {@path} {n}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A check with no parameter: `check nonempty: test -s {@path}` in [stats.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/stats.spit)
- A check on a source: `source reading : Readings<Raw> .csv [region, site, day] @check(rows(5))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A check on an input port: `series: Readings<$Kind> @check(nonempty)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A check on an output: `spread: Mean @check(plausible(30))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A check every output runs: `default check: nonempty` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A default check that an output drops: `@check(!stats::nonempty)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
