# Operations

An **operation** is a reusable piece of work: it says what a job reads, what it makes, and which command makes it. A [step](pipelines.md#steps) applies an operation to products, and each job SPIT plans is one run of an operation on one set of artifacts. This page covers how operations are declared, how their commands are written, and how to declare what a job needs and how to test it.

## Signatures

A signature lists the inputs a job takes and the outputs it makes:

```spit
operation sort_lines(input: Lines) -> Lines
operation merge(items: many Lines) -> Lines
```

- Every input is a **port** with a name, and the name is its placeholder in the command. A port is written `name`, `name: Type`, `name: many` or `name: many Type`.
- `sort_lines` takes one `Lines` artifact for each job. `merge` takes a **collection** of them, so its port says `many`. Which dimensions the collection gathers is decided by the step, with `@vary(...)`.
- A call's arguments follow the port order, and SPIT checks each argument's type against its port.
- A port can carry a bound on its collection. `items: many Lines @min(2)` rejects a group with fewer than two artifacts.

An operation that makes several outputs names each one. The step assigns one product to each, in order, and the command refers to the files by their names:

<!-- spit-doc: setup
source series : Series [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation split(series: Series) -> (low: Series .csv, high: Series .csv)
command split: split_tool {series} --low {low} --high {high}
low_band, high_band = split(series)
```

A command must use every output it makes. The one exception is an output declared [`beside`](artifacts.md#companion-outputs) another.

An operation that no step calls is not checked until a step calls it, so a file of operations to import into other pipelines needs no data of its own. An operation can also be carried out by steps instead of a command: see [composite operations](pipelines.md#composite-operations).

## Command templates

A `command` line gives an executable and its arguments. SPIT fills in the placeholders for each job and records the result in the plan:

<!-- spit-doc: setup
operation sort_lines(input: Lines) -> Lines
-->

```spit
command sort_lines: sort -u -o {@output} {input}
```

For `shard[group=alpha,part=01]`, the plan holds this command, which `spit dag --commands` shows:

<!-- spit-doc: output
spit dag examples/runnable/lines/lines.spitin --commands
-->

```text
Job 1  sort_lines
  run:    sort -u -o output/sorted/group=alpha_part=01.txt input/alpha/01.txt
```

`{input}` is the path of the `input` port's artifact, and `{@output}` is the path of the operation's one unnamed output. A named output uses its own name, as in `{low}` above.

- The first word must be an executable on `PATH`, or a path to one. SPIT does not install tools or load shell functions.
- A command is a list of words, not a shell script. Quote text that has spaces, as in `--title "my plot"`. A substituted path always stays one argument, whatever characters it holds.
- Shell syntax such as `|`, `&&`, `;` and `$VARIABLE` is not expanded. To use it, run a shell yourself and pass the paths as arguments: `sh -c 'cut -f1 "$1" | sort -u > "$2"' sh {table} {@output}`.
- A command can wrap over several indented lines, and the operation's signature can too, while its brackets are open.

### Placeholders

| Form | Value |
| --- | --- |
| `{input}` | The path of an input port's artifact |
| `{items}` | A `many` port: one argument for each artifact, in collection order |
| `{@output}` | The path of the single unnamed output |
| `{low}` | The path of the output named `low` |
| `{image.dir}`, `{image.stem}` | An output's folder, and its file name without the extension |
| `{group}` | The value of the job's `group` dimension, as text |
| `{items.part}` | A `many` port: each artifact's `part` value, one argument each |
| `@join(items)` | A `many` port's paths as one argument, separated by `,` |
| `@list(items)` | The path of a file that lists a `many` port's paths, one to a line |
| `@scalar(count, min=1)` | The number held in the `count` input's file |
| `{@cores}` | The number of cores the job was given |
| `{@here}/scripts/sort.sh` | A script beside the file that declares the operation |

The sections below say more about the ones that need it.

**Job values.** A placeholder that names a dimension gives the job's value as text, which suits a read group or a plot title:

<!-- spit-doc: setup
source image : Image [sub, ses]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation plot(input: Image) -> Figure .png
command plot: plot_tool {input} --title "subject {sub}, session {ses}" --out {@output}
figure = plot(image)
```

The job for `image[sub=01,ses=2]` runs `plot_tool ... --title 'subject 01, session 2' ...`. A job has the dimensions of its outputs, so a step that collects sessions with `@vary(ses)` has no single `{ses}`. Use `{items.ses}` for each collected artifact's value. See [a job's dimension values](../language-reference.md#a-jobs-dimension-values).

**Collections as one argument.** `{items}` gives a tool one argument for each artifact. A tool that wants them in a single argument, or in a file, uses `@join(items)` or `@list(items)`:

<!-- spit-doc: setup
source scan : Image .nii.gz [sub, run]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation average(images: many Image) -> Image .nii.gz
command average: mean_tool --inputs=@join(images) -o {@output}

operation register(images: many Image) -> Table .tsv
command register: register_tool --file-list @list(images) -o {@output}

averaged = average(scan @vary(run))
registered = register(scan @vary(run))
```

For `sub=01` with runs 2 and 10, `average` runs `mean_tool --inputs=out/scan/sub=01_run=2.nii.gz,out/scan/sub=01_run=10.nii.gz -o ...`. Both forms list artifacts in collection order, so run 2 comes before run 10. See [a many input as one argument](../language-reference.md#a-many-input-as-one-argument).

**Numbers from files.** When a tool needs a number that another tool prints, write the number to a file in one step and read it into a later command with `@scalar`:

<!-- spit-doc: setup
source image : Image .nii.gz [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation read_tr(input: Image) -> Scalar .txt
command read_tr: fslval {input} pixdim4 > {@output}
tr = read_tr(image)

operation fit(tr: Scalar, input: Image) -> Map .nii.gz
command fit: fit_tool --tr @scalar(tr, min=5) {input} --out {@output}
map = fit(tr, image)
```

When a `fit` job starts, the runner reads that subject's `tr` file and runs `fit_tool --tr 6.234000 ...`. A file that holds anything but one number, or a number under `min`, fails the job before `fit_tool` starts. See [numbers read from files](../language-reference.md#numbers-read-from-files).

**Scripts beside the pipeline.** Commands run from the output root, so a script kept next to the pipeline cannot be named by an ordinary relative path. Declare it as a `here` source, which SPIT finds from the pipeline's folder, and pass it to the operation. A job that reads it reruns when the script changes:

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

A script that is not an input can be named with `{@here}/scripts/table.py` instead, but editing it then reruns nothing. [Files beside the pipeline](../language-reference.md#files-beside-the-pipeline) covers both.

### Standard input and output

A tool that prints its result instead of writing a file is given `>` and an output placeholder, and one that reads standard input is given `<` and an input placeholder:

<!-- spit-doc: setup
source table : Table [sheet]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation first(table: Table) -> Column .tsv
command first: cut -f1 {table} > {@output}

operation count(table: Table) -> Count .txt
command count: wc -l < {table} > {@output}

column = first(table)
lines = count(table)
```

The runner opens the files and connects them, so no shell is involved. `<` and `>` are words of their own, followed by one placeholder, and a command has at most one of each. Only an operation's `command` can use them. A `verify` or a `check` that needs a redirect runs a shell.

### Several commands in one job

A job sometimes needs two tools in turn, as `samtools index` finishes what `samtools sort` wrote. Leave the `command` line empty after its `:` and put each command on its own indented line. They run in order and stop at the first that fails:

<!-- spit-doc: setup
source sam : Sam .sam [sample]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation sort(sam: Sam) -> (bam: Bam .bam, bai: Bai +.bai beside bam)
resources sort: cores=4
command sort:
    samtools sort --threads {@cores} -o {bam} {sam}
    samtools index {bam}
bam, bai = sort(sam)
```

`spit dag --commands` shows a `run:` line for each. Every placeholder works on every line, and between them the lines must use every output. See [commands of several lines](../language-reference.md#commands-of-several-lines).

## Tools

A runner reruns a job when its command or its files change, but it cannot see a tool's version, so a job made with an old version of a tool still looks current after an upgrade. A recipe fixes this by naming a `.spitenv` file that records what each tool is on the machine where the jobs run:

<!-- spit-doc: recipe
--- lines.spit
source shard : Lines [group, part]
path shard: input/{group}/{part}.txt
default entities: {@key}={@value} separated "_"
default path: output/{@product}/{@entities}.txt
tool sort_tool
operation sort_lines(input: Lines) -> Lines
command sort_lines: sort -u -o {@output} {input}
uses sort_lines: sort_tool
sorted = sort_lines(shard)
--- tools.spitenv
tools:
    sort_tool: probe sort --version
    other_tool: docker.io/lab/tool@sha256:4f1c
-->

```spit
pipeline lines.spit
root .
tools from tools.spitenv
```

The `.spitenv` has one line for each tool, `name: identity`. An identity is either fixed text, such as a version or a container digest, or `probe` followed by a command, whose output is the identity. The runner runs each probe once, before any job, and a probe that fails stops the run.

- With no `uses` line, every job depends on every tool in the `.spitenv`. That never misses a rerun, and may cause a few that were not needed.
- `tool sort_tool` declares a tool in the pipeline, so that `spit check` can catch a misspelled name. `uses sort_lines: sort_tool` limits `sort_lines` jobs to that tool.
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

## Resources

A runner starts several jobs at once. A `resources` line says what each job of an operation needs, so it starts only as many as the machine can hold:

<!-- spit-doc: setup
source reads : Fastq [sample]
source reference : Fasta
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation align(reference: Fasta, reads: Fastq) -> Sam .sam
command align: bwa mem -t {@cores} {reference} {reads} > {@output}
resources align: cores=4, memory=8G
aligned = align(reference, reads)
```

Each `align` job runs `bwa mem -t 4 ...`, and the runner starts one only when four cores and eight GiB of memory are free. Write `{@cores}` where the tool takes its thread count, so the tool uses what the runner set aside.

- `cores=N` is a whole number from 1, and `memory=SIZE` is a whole number with a unit `K`, `M`, `G` or `T`, each 1024 times the one before. Give either or both.
- An operation without a `resources` line needs one core and claims no memory, and `{@cores}` is `1`.
- `spit-bash` stops a job that holds more memory than it declares, so declare the most the job needs on your largest input.
- A changed `resources` line does not make a finished job out of date, unless the command's `{@cores}` changes with it.

`spit dag --commands` shows a `needs:` line above the `run:` line of each job that declares resources. See [cores and memory](../language-reference.md#cores-and-memory).

## Checks and verification

SPIT does not run tests of its own. It writes the commands you declare into the plan, and the runner runs them. A failed test fails the job and stops the jobs that depend on it.

### Verification

`verify` tests a job's **inputs** before its main command. Use it for a condition that involves more than one input, such as whether two files have the same grid:

<!-- spit-doc: setup
source moving : Image [sub]
source reference : Image [sub]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
operation register(moving: Image, reference: Image) -> Image
command register: reg_tool {moving} {reference} {@output}
verify register: check_same_grid {moving} {reference}
registered = register(moving, reference)
```

A `verify` command may use any input port, including a `many` port, and `{@cores}`. If it fails, the main command does not run. `spit dag --commands` lists each job's `verify` lines above the command they guard.

### Artifact checks

A `check` tests **one artifact**. Declare it once and attach it with `@check(...)`:

<!-- spit-doc: setup
source shard : Lines [group, part]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
check nonempty: test -s {@path}
operation sort_lines(input: Lines) -> Lines @check(nonempty)
command sort_lines: sort -u -o {@output} {input}
sorted = sort_lines(shard)
```

`{@path}` is the path of the artifact being tested, and the check must use it. In this example the runner tests each sorted file after writing it. Where `@check(...)` goes decides when it runs:

- On an **output**, as above, it runs after the command, before the job counts as done.
- On an **input port** or a **source**, it runs on each artifact the port reads, before the job's `verify` commands and command.
- A check can take parameters, written where it is declared and where it is attached: `check ndim(n): check_ndim {@path} {n}` and `@check(ndim(3))`.
- `default check: nonempty` applies a check to every output in the file or stage. `!nonempty` drops an inherited one.

A check reads only its own artifact and never adds a dependency between jobs. When the job that writes an artifact checks it, a job in the same plan that reads it does not run the same check again. See [checks](../language-reference.md#checks) and [default checks](../language-reference.md#default-checks).

Next: [Pipelines](pipelines.md).
