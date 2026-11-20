# Operations

An **operation** is a reusable piece of work: it says what a job reads, what it makes, and which command makes it. A [step](pipelines.md#steps) applies an operation to products, and each job SPIT plans is one run of an operation on one set of artifacts. This page covers how operations are declared, how their commands are written, and how to declare what a job needs and how to test it. The examples come from the [showcase analyses](https://github.com/eclnz/spit-showcase), such as the [weather network](walkthrough.md).

## Signatures

A signature lists the inputs a job takes and the outputs it makes. The weather network's cleaning step reads many day files and one calibration, and makes one series:

```spit
operation clean(days: many Readings<Raw> @min(2), calibration: Calibration) -> Readings<Clean>
```

- Every input is a **port** with a name, and the name is its placeholder in the command. A port is written `name`, `name: Type`, `name: many` or `name: many Type`. Type names start with a capital letter, so a lowercase word alone, as in `operation copy(image)`, is an untyped port.
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

A command must use every output it makes, by its placeholder, its `.dir`, `.stem` or `.prefix`, or as its [standard output](#standard-input-and-output). The one exception is an output declared [`beside`](artifacts.md#companion-outputs) another. `output` cannot name a port, and `{output}` is an error that points to `{@output}`.

A long signature can be laid out over several lines while its brackets are open. Indent the ports beyond the `operation` line, keep `->` and the output list's `(` on the line that closes the inputs, and put no comma after the last port:

<!-- spit-doc: plain -->

```text
operation prep_registration(
    image: MRI<$I,$Space,$Grid>,
    reference: MRI<$I,$Space,$Grid>
) -> (
    parc: MRI<Parc,$Space,SynthGrid<$Grid>>,
    resampled: MRI<$I,$Space,SynthGrid<$Grid>>
)
```

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
- A command is a list of words, not a shell script. Words are split and quoted as in Bash: quote text that has spaces, as in `--title "my plot"`, and text in single quotes is literal, so `awk '{print $1}'` needs no escaping. A placeholder is filled in unquoted text or in double quotes, and a substituted path always stays one argument, whatever characters it holds. Write `{{` or `}}` for a literal brace elsewhere.
- Nothing is expanded: `$VARIABLE` and backticks reach the program as written. An unquoted `|`, `>>`, `&&`, `;` or `2>&1` is an error, since no shell runs the command; quote it, as `'|'`, to pass it as an argument. To use a pipe, run a shell yourself and pass the paths as arguments, so that a path is never read as shell text, as the weather network's `day_of_hours` check does: `sh -c 'test "$(wc -l < "$1")" -eq 25' sh {@path}`.
- Every command is checked when the pipeline is loaded: its braces and quotes balance, each placeholder names a port or a [dimension of the job](#a-jobs-dimension-values), and each output is used.
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
| `{seg.prefix}` | An output's path without its [naming suffix](artifacts.md#outputs-named-after-a-prefix) and extension |
| `{alert}` | The value of the job's `alert` dimension, as text |
| `{days.day}` | A `many` port: each artifact's `day` value, one argument each |
| `@join(images)` | A `many` port's paths as one argument, separated by `,` |
| `@list(images)` | The path of a file that lists a `many` port's paths, one to a line |
| `@scalar(size, min=1)` | The number held in the `size` input's file |
| `{@cores}` | The number of cores the job was given |
| `{@here}/steps/reduce.py` | A script beside the file that declares the operation |

A tool that takes a folder and a name, and adds the extension itself, is given an output's folder with `{image.dir}` and its file name without the extension with `{image.stem}`. For `derivatives/image/sub=01.nii.gz`, they are `derivatives/image` and `sub=01`. `.stem` needs the output to declare its [extension](artifacts.md#extensions), so that SPIT knows where the name ends, unless the output is a [folder](artifacts.md#folders), whose stem is its whole name. A file at the root is in folder `.`. Only outputs have `.dir`, `.stem` and `.prefix`, `{@output.dir}` and `{@output.stem}` are the unnamed output's, and any other `.` part is an error.

The sections below say more about the others.

#### A job's dimension values

A placeholder that names a dimension gives the job's value as text. The weather network's tally job names its alert, and the variant-calling analysis writes each sample's name into the read group:

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

A job has the dimensions of its outputs: those of the input that drives it, less those a `many` input collects, with those an input broadcasts. In the editor, a hover on the step's product shows them. So a step that collects with `@vary(day)` has no single `{day}`, and `spit check` says so at the placeholder:

<!-- spit-doc: fragment -->

```spit
source seconds : Duration [speaker, digit]
operation table(durations: many Duration) -> Table
command table: table --digit {digit} {durations} -o {@output}
summary = table(seconds @vary(digit))
```

<!-- spit-doc: output
spit check v.spit || true
--- v.spit = @previous
-->

```text
error: line 3, column 30: command for `table` uses `{digit}`, but `durations` collects `seconds` across `digit` with `@vary(digit)`, so each job has many values of it; `{durations.digit}` gives each artifact's, and the jobs of `summary` have `speaker`
```

Use `{days.day}` for each collected artifact's value, one argument for each artifact in the order `{days}` gives their paths. The weather network's cleaning command uses it to hand the script each file's date. On a single-artifact input, `{location.site}` is that artifact's value, which reaches a dimension the job lacks, such as one pinned with `@where`.

- The value is text within its argument. It never splits an argument or adds one.
- In single quotes a placeholder is literal text, so write an argument that needs quotes in double quotes, as the read group above is.
- A port's name comes first: in `operation train(model: Weights, ...)`, `{model}` is the port's file even when the job has a `model` dimension. Rename the port to give the tool the value.
- The value is not a file, so it has no `.dir` or `.stem`, and `<` and `>` cannot name it. `verify` commands may use it; checks may not.

In the [weather family](weather.md):

- A job's own value in a command: `-v site={site}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- Each artifact's value of a dimension: `{means.site}` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

#### Collections as one argument

`{days}` gives a tool one argument for each artifact. A tool that wants them in a single argument, or in a file, uses `@join(...)` or `@list(...)`:

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

Both forms list artifacts in collection order, so run 2 comes before run 10.

- `@join` separates the paths with `,`, or with the text in double quotes after a comma: `@join(images, " ")`, or `""` for none. Write `\"`, `\\`, `\n`, `\r` or `\t` for a quote, a backslash, a newline, a carriage return or a tab. A path that holds the separator is an error, since the tool could not split them apart again.
- `@join(images.run, ";")` joins each artifact's value of a dimension instead.
- `@list` writes each path whole, one to a line. The runner writes the file when the job starts, gives the same file to each of the job's commands, and removes it when the job ends. Where it is written is not part of the job's work.
- Both name a `many` port, and go in a `command` or `verify` command, alone or within an argument, but not as the program, nor after `<` or `>`. In quotes, as `'@join(images)'`, either is plain text.

In the [weather family](weather.md):

- A collection as one argument: `@join(means)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- A collection as a file of paths: `@list(means)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

#### Numbers from files

When a tool needs a number that another tool prints, write the number to a file in one step and read it into a later command with `@scalar`:

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

When the second job starts, the runner reads the number in `size`'s file and runs `awk -v n=3 ...`. A file that holds anything but one number, or a number under `min`, fails the job before `awk` starts, so an empty list never reaches it.

- `@scalar(port)` names a single-artifact input whose artifact is a file. It goes in a `command` or `verify` command, alone or within an argument, as in `--tr=@scalar(tr)`, but not as the program, nor after `<` or `>`.
- The file holds one finite decimal number, such as `6.234000`, `-2`, `.5` or `1e-3`, with only whitespace around it. The tool gets the text as the first tool printed it, trimmed.
- `min=N` and `max=N` are the least and greatest values allowed, both included: `@scalar(tr, min=5, max=100)`.
- The number stays in its file, so a job reruns when the file changes. In quotes, `'@scalar(tr)'` is plain text.

In the [weather family](weather.md):

- A number read from a file: `@scalar(offset)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

## Calculating numbers

```spit
source reference : Scalar .txt [sub]
source gain : Scalar .txt [sub]
path reference: reference/{sub}
path gain: gain/{sub}
default path: out/{@product}/{sub}

operation calibrate(reference: Scalar, exp: Scalar) -> Scalar .txt
calculate calibrate: round(reference * exp(exp), 1)
m0 = calibrate(reference, gain)

operation report(m0: Scalar) -> Text .txt
command report: printf '%s\n' @scalar(m0, min=0) > {@output}
result = report(m0)
```

For a subject whose input files hold `80` and `1.844439727`, the calculation writes `506.0` followed by a newline. The report reads that file with `@scalar`.

- Use one `calculate operation: expression` in place of the operation's `command`. The operation has one file output. Its usual input checks, `verify` commands and output checks still apply.
- A bare name reads one finite decimal from the file at that input port. A name followed by parentheses calls a function, so `exp(exp)` calls `exp` on the number from port `exp`.
- Use decimal literals such as `2`, `.5` and `1e-3`. The operators bind from tightest to loosest as follows: parentheses, exponentiation `^`, unary signs, multiplication and division, then addition and subtraction. Exponentiation groups from the right, so `2^3^2` is `2^(3^2)`. A negative exponent is allowed.
- The functions are `exp(x)`, `log(x)` (natural logarithm), `sqrt(x)`, `abs(x)`, `min(x, y)`, `max(x, y)` and `round(x, places)`. Round to the nearest decimal with ties to even. The places must be an integer from 0 to 15. When the outermost operation is `round`, the file preserves the requested decimal places.
- Invalid input, invalid arithmetic and non-finite results fail the calculation job and block its dependents. Put bounds on the number at its point of use with `@scalar`.


#### Scripts and settings as inputs

A job reruns when a file it reads changes, so a script or a settings file that decides the result should be an input. The terrain analysis lists its measuring script and each region's grid options as sources, and the classifier analysis does the same for each step's script:

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

- A `here` source is one file, found from the folder of the `.spit` file that declares it, so a library in `libs/` keeps its scripts there. It takes no dimensions, `/` or `beside`, and must be in that folder or beneath it.
- Nothing may be written beside a `here` source, and it must exist when the pipeline runs.
- A pipeline read from standard input has no folder, so a `here` source in it is an error.

The weather and quality-control pipelines name their scripts with `{@here}` instead, the folder of the file that declares the operation, at the start of an argument. That works from any folder, but a job declares only its ports, so editing such a script reruns nothing. `spit dag` notes each one so the choice is deliberate:

```text
note: operation `table` runs `{@here}/scripts/table.py`, which it doesn't declare, so editing the file reruns nothing; to rerun on a change, declare it a `here` source and pass it to `table` as a source
```

In the [weather family](weather.md):

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

The runner opens the file and connects the tool's output to it, so no shell is involved. `tr a-z A-Z < {list} > {@output}` uses both.

- `<` and `>` are words of their own, each followed by one placeholder: `>{@output}` and `> out.txt` are errors. A command has at most one of each.
- The output on `>` is a file, not a folder, and no other argument names it, not even its `.dir` or `.stem`. The input on `<` is a single-artifact input that is a file.
- Only an operation's `command` can redirect. A `verify` or a `check` that needs to runs a shell.
- What a tool writes to standard error is left to the runner, which logs it.

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

`spit dag --commands` shows a `run:` line for each.

- Every placeholder works on every line, and between them the lines must use every output. A line may name an output an earlier line wrote.
- Each line may read on `<`, and one line at most writes on `>`. The lines after it may name that output; the lines before it may not.
- A line indented further than the first carries on the line above it. In a `command:` block, an entry ending in `:` holds its lines beneath it, as `order:` does in the weather family.
- A `verify` or `check` line holds one command. Give each `verify` command a line of its own: an operation's `verify` lines run in turn.

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

The `.spitenv` has one line for each tool, `name: identity`. An identity is either fixed text, such as a version or a container digest, or `probe` followed by a command, whose output is the identity. The runner runs each probe once, before any job, with no shell, and a probe that fails stops the run.

Choose an identity that changes when the tool does. Version flags are uneven: `flirt -version` gives only `FLIRT version 6.0`, and `bet` has none. A conda environment's lockfile or a container's digest identifies a tool exactly. A probe can also check what it reports, as `sh -c 'grep -qx 6.0.7.23 "$FSLDIR/etc/fslversion" && conda list -p "$FSLDIR" --explicit --md5'` does. SPIT does not guess tools from command names: `dwifslpreproc` is an MRtrix3 program that runs FSL.

- With no `uses` line, every job depends on every tool in the `.spitenv`. That never misses a rerun, and may cause a few that were not needed.
- `tool python` declares a tool in the pipeline, so that `spit check` can catch a misspelled name, and the recipe's `.spitenv` must then list every declared tool.
- `uses joined: python` limits `joined` jobs to that tool. The first name replaces "every tool" with a list, and lists then add up: the file's `default uses:`, each stage's from the outermost, then the operation's own line. `!awk` removes a tool.
- An operation's tools follow where it is declared, not where it is called. An imported operation keeps the tools its own file gives it.
- A pipeline planned without a recipe, or from a `.spitout`, has no `.spitenv`, so its jobs depend on no tool.
- When a tool's identity changes, the runner reruns the jobs that depend on it. To accept an upgrade that should not rerun finished work, use [`spit-bash accept-tools`](runs.md#reuse).

`spit check runtime.spitenv` checks a `.spitenv` alone, and `spit check dataset.spitin` checks it against the pipeline.

In the [weather family](weather.md):

- The tools a recipe records: `tools from ../shared/network.spitenv` in [v4_survey.spitin](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v4_survey/v4_survey.spitin)
- A tool's identity from a probe: `python: probe python3 --version` in [network.spitenv](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/network.spitenv)
- A tool's identity as fixed text: `awk: system-awk` in [network.spitenv](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/network.spitenv)
- A tool declared in the pipeline: `tool:` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- The tools an operation depends on: `write: python, !awk` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)
- The tools every operation depends on: `uses: awk` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

### Tools that store their paths

A runner identifies a job by what it makes and runs, not by where its files are, so moving the dataset leaves finished jobs current. Some tools write their own paths into an output, so a moved file points at a place that does not exist. FSL FEAT's `design.fsf` is one, and a group analysis's `.gfeat` names its first-level folders. Mark such an operation with `anchored`:

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

An anchored job reruns when the path of a file it reads or writes changes, or the dataset's folder does, and the runner never moves its outputs.

- `anchored` names operations the file declares, separated by commas, or one to a line of an `anchored:` block. The mark goes wherever the operation is called, and an imported operation keeps its own.
- Tools that only record how an output was made, such as the `@PG` lines of `bwa mem` or MRtrix3's `command_history`, need no mark.
- A `{x.dir}` or `{x.stem}` in a command is already part of the job's work, so it needs no mark either.

In the [weather family](weather.md):

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
- An operation has one `resources` line, wherever it is written, and an imported operation brings its own. A composite operation has none: each operation its body calls has its own.
- `spit-bash` stops a job that holds more memory than it declares, so declare the most the job needs on your largest input.
- A changed `resources` line does not make a finished job out of date, unless the command's `{@cores}` changes with it.

`spit dag --commands` shows a `needs:` line above the `run:` line of each job that declares resources.

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

### Artifact screens

```spit
screen usable: test -s {@path}
source data : Lines [sub] @screen(usable)
operation summarise(input: Lines) -> Summary
summaries = summarise(data)
path summaries: output/{sub}.txt
```

A screen decides whether readers use an artifact. The runner checks the artifact, then runs the screen and excludes it if the command exits nonzero. A human verdict overrides a screen at the same attachment. An artifact check failure still fails execution.

Declare parameterized screens with `screen usable(limit): command` and attach them with `@screen(usable(35))`. A screen on a source or output gates every reader. A screen on an input port gates that use alone. To gate one call, write `summaries = summarise(data @screen(usable))`. Call-specific `@check`, `@screen` and `@review` attachments add to the ones already on the port and product. A call to an operation carried out by steps passes its attachments to the steps that read the argument. Use `screen:` for a block of declarations, or `default screen: usable` to screen every output in a file or stage. An output can opt out with `@screen(!usable)`.

The runner records outcomes in a `.screens.csv` for each screened product. The same saved DAG can be run after changes to a human review CSV. [Human review](datasets.md#human-review)

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

`{@path}` is the path of the artifact being tested, and the check must use it and every parameter, and nothing else. Each `{min_snr}` is the word given where the check is attached, so `usable(35)` runs the script with the summary's path and `35`. An argument is one word, without spaces, quotes, braces, commas or parentheses, and a check with no parameters has no parentheses, where it is declared and where it is attached. A subject whose signal-to-noise ratio is under 35 fails `approve`, and so does every job that reads `approved`, while the other subjects, and the table of every subject, go on. The digits analysis declares a check that a trimmed recording still holds audio, and attaches it to the output, `@check(audible)`. Where `@check(...)` goes decides when it runs:

- On an **output**, as above, it runs after the command, before the job counts as done.
- On an **input port** or a **source**, it runs on each artifact the port reads, before the job's `verify` commands and command. The weather network attaches `@check(day_of_hours)` to its readings.
- On a `many` port, it runs on each artifact of the collection.
- `@check(nonempty, ndim(3))` attaches several, and all of them run. A step's product takes no checks: attach them to the operation's output.

A check reads only its own artifact and never adds a dependency between jobs. When the job that writes an artifact checks it, a job in the same plan that reads it does not run the same check again. Checks are global, as operations are: `use` brings in the checks of what it imports, and `use ndim from checks.spit` imports one by name. `spit dag --commands` shows each job's checks as `check:` lines, in the order they run.

In the [weather family](weather.md):

- A check on one artifact: `check rows(n): sh -c 'test "$(wc -l < "$1")" -eq "$2"' sh {@path} {n}` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A check with no parameter: `check nonempty: test -s {@path}` in [stats.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/shared/stats.spit)
- A check on a source: `source reading : Readings<Raw> .csv [region, site, day] @check(rows(5))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A check on an input port: `series: Readings<$Kind> @check(nonempty)` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A check on an output: `spread: Mean @check(plausible(30))` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)

### Default checks

When every output in a file or stage needs the same check, write `default check:` once instead of `@check(...)` on each output:

<!-- spit-doc: setup
source raw : Image [sub]
operation denoise(input: Image) -> Image
command denoise: dn {input} {@output}
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
-->

```spit
check nonempty: test -s {@path}
check ndim(n): check_ndim {@path} {n}

default check: nonempty

stage preprocess:
    default check: ndim(3)
    cleaned = denoise(raw)
```

`cleaned` runs `nonempty`, then `ndim(3)`.

- A default covers every output of every step in its file or stage, and never an input port or a source. A default written for an extension, as below, does cover sources.
- A step takes the defaults of the stage it is in, not of the stage that declares its operation.
- Lists add up: the file's checks, then each stage's from the outermost, then those the output names. A check that would run twice on an artifact runs once.
- `!` drops a check a wider list added. In a stage, `default check: !nonempty` drops it for that stage and the stages in it. On an output, `-> (empty_ok: Table @check(!nonempty), rows: Table)` drops it for that output wherever the operation is called. An output's own `@check(nonempty)` is never dropped.

#### Checks by extension

A file's checks can follow its format. `default check .nii.gz:` runs its list on every file whose whole extension is `.nii.gz`, whatever its type, and on the sources of that extension too, before the job that reads them:

```spit
source t1w : Image .nii.gz [sub]
source events : Table .tsv [sub]

check nifti_ok: test -s {@path}
check tidy: test -s {@path}

default check .nii.gz: nifti_ok
default check .tsv: tidy

operation extract(image: Image) -> (brain: Image .nii.gz, mask: Mask .nii.gz beside brain suffix "_mask", notes: Text .txt)
command extract: extract_brain {image} {brain} {notes}
default path: out/{@product}/{sub}
path t1w: in/{sub}
path events: events/{sub}
brain, mask, notes = extract(t1w)
```

`t1w` runs `nifti_ok` before `extract`, and `brain` and `mask` run it after. `notes` and `events` have no list for `.txt`, so `events` runs `tidy` and `notes` runs nothing.

- The extension is the whole one: `.nii.gz` does not cover a `.gz` file or a `.nii` file, so each needs its own list. A file written beside another has the extension its suffix gives. A folder has none.
- A scope has one list for each extension. Lists for an extension add up through the stage and file as `default check:` lists do, after the scope's own list without an extension.
- `!` drops a check, in a list or on one file: `-> (raw: Image .nii.gz @check(!nifti_ok))` for an output, and `source scratch : Image .nii.gz [sub] @check(!nifti_ok)` for a source.

In the [weather family](weather.md):

- A check every output runs: `default check: nonempty` in [v2_checked.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v2_checked/v2_checked.spit)
- A default check that an output drops: `@check(!stats::nonempty)` in [v3_scenarios.spit](https://github.com/eclnz/spit/blob/dev/examples/runnable/weather/v3_scenarios/v3_scenarios.spit)

Next: [Pipelines](pipelines.md).

