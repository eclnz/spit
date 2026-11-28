# spit-bash

`spit-bash` runs the jobs of a [SPIT](../README.md) pipeline on one machine, or as Slurm jobs on a cluster (see [Running on a Slurm cluster](#running-on-a-slurm-cluster)). Give it a recipe and it runs `spit dag` for you; give it a saved `.spitdag` or `dag --json` output and it needs nothing else, not the original `.spit` pipeline or `.spitout` inventory. It reads one version of [SPIT's DAG format](../docs/spitdag.md), and no other: a DAG of another version is rejected before anything runs, so use a `spit` and a `spit-bash` that match. `spit-bash --dag-version` and `spit --dag-version` print the version each uses.

See the [runnable examples](../examples/runnable/README.md) for recipes to run, their commands, and expected output.

## Install and use

Requires Python 3.10 or newer, and `spit` on `PATH` to run a recipe or pipeline. From the repository's root, install it with [uv](https://docs.astral.sh/uv/), which puts `spit-bash` on your `PATH` in an environment of its own, or without uv with `python3 -m pip install ./runner`:

```sh
uv tool install ./runner
spit-bash plan dataset.spitin
spit-bash run dataset.spitin -j 4
```

`spit-bash --version` (`-V`) prints the installed runner's release version. `spit-bash --dag-version` prints the `.spitdag` format version it reads. Compare it with `spit --dag-version` to check whether the compiler writes that format.

`spit-bash` takes what `spit dag` takes, and runs `spit dag --json` on it:

| Files | Runs |
| --- | --- |
| `dataset.spitin` | `spit dag dataset.spitin --json` |
| `analysis.spit` with a `root` line | `spit dag analysis.spit --json`: a small pipeline names its dataset root itself, so it needs no `.spitin` |
| `analysis.spit dataset.spitout` | `spit dag analysis.spit dataset.spitout --json` (`-` for a `.spitout` on stdin) |
| `analysis.spit --root data` | `spit dag analysis.spit --root data --json`, scanning `data` |

`--partial` is passed on to `spit dag`, which leaves out the jobs it cannot complete, and any aggregate with a member among them; with `run`, it also runs the jobs that are not blocked (see [Planning and running](#planning-and-running)). `--spit PATH`, or the `SPIT` variable, names the `spit` program when it is not on `PATH`. SPIT's notes and errors appear as it prints them, and if `spit dag` fails, `spit-bash` stops with status 2 before planning anything.

A saved DAG, or one on stdin, is read directly:

```sh
spit dag dataset.spitin -o jobs.spitdag
spit-bash run jobs.spitdag -j 4
spit dag dataset.spitin --json | spit-bash run - -j 4
```

A saved DAG is a snapshot of the pipeline it was made from. It records each of the pipeline's files with its git blob id, so `plan` and `run` compare them with the files on disk, and warn when one has changed or gone, as `warning: the pipeline changed since this DAG was written: digits.spit. Run the recipe, or spit dag again.` The DAG still runs as written. A DAG made from a recipe in the same command is never stale, so it is not compared.

## Choosing jobs

`--entity`, `--product` and `--stage` choose part of the DAG: the jobs whose outputs match, and every job upstream that they need. Nothing downstream is chosen. Try a pipeline on one subject before running every subject:

```sh
spit-bash run dataset.spitin --entity sub=01
spit-bash run dataset.spitin --entity sub=01,ses=02 --product denoised_dwi
spit-bash plan dataset.spitin --stage preprocess
```

| Option | Chooses jobs with |
| --- | --- |
| `--entity DIM=VALUE[,...]` | an output with one of the listed values for each dimension |
| `--product NAME[,...]` | an output of one of the listed products |
| `--stage NAME[/NAME...][,...]` | one of the listed stages, or a stage inside it, such as `preprocess/combine` |

Each option takes a comma-separated list, and can also be repeated, and both mean the same: an output matches when it matches any of the values, so `--product clean,mask --stage preprocess,register/combine` is `--product clean --product mask --stage preprocess --stage register/combine`. A `/` names a stage inside a stage, and an empty entry, as in `clean,,mask`, is an error.

After a dimension's first value, each bare value belongs to that dimension until the next `DIM=VALUE`. For example, `--entity sub=01,02,ses=baseline` chooses either subject in the baseline session. Repeat `--entity` to match specific combinations, as in `--entity sub=01,ses=baseline --entity sub=02,ses=followup`.

```sh
spit-bash plan examples/runnable/lines/lines.spitin --entity group=alpha,beta
```

Different options must all hold for the same output, so `--entity sub=01 --product clean` chooses the jobs that make `clean` for subject 01. A job with no `sub` dimension, such as one making a group template, is not chosen by `--entity sub=01`, but it runs if a chosen job needs it; a group average over every subject is not chosen, so it does not run. `plan` and `run` print how many jobs were chosen and how many more they need. A name that matches no output is an error that lists the names that would, so a misspelt dimension, value or product does not just choose nothing.

If selected work waits for a review with declared evidence, the runner includes the jobs that make that evidence. Once the candidate and evidence are ready, the runner reports the inspection path and fills it into the verdict sheet. The local runner and Slurm use the same selection and review rules.

Choosing jobs does not change how a job is planned: a chosen job is still skipped when it is current, and the state records it as any run does. Jobs outside the choice are not run, so one that reads an output a chosen job rewrote may be out of date afterwards. `run` judges each as `plan` would, by stamp and then by content, and counts those whose record the rewritten output no longer matches in a line after its summary, as `10 jobs outside the selection read outputs this run changed; run without the selection to bring them up to date`. When every rewritten output holds the bytes its readers' records do, it says so instead, as `3 outputs were rewritten with the same content; nothing outside the selection needs to run`. Under `--compare stat` a rewritten output always counts as changed.

`--force` with a selection reruns only the jobs whose outputs matched it. Their prerequisites still run when stale and skip when current:

```sh
spit-bash run analysis.spitin --entity sub=01 --force
```

Without `--entity`, `--product` or `--stage`, `--force` reruns every command job. A downstream job that reads an output rewritten by a forced job is outside a partial selection, and the usual stale-job count reports it.

For a stage whose inputs are already on disk, add `--no-upstream` to `plan` or `run`:

```sh
spit-bash plan dataset.spitin --stage report --no-upstream
spit-bash run dataset.spitin --stage report --no-upstream
```

Only jobs that match the selection can run. They still wait for any selected jobs that produce their inputs. For inputs made by jobs outside the selection, the runner uses the files at their declared paths and reports any missing files as blocked inputs. A collection still requires every member after verdicts are applied. If a selected job needs a screen verdict from an excluded producer, include the upstream jobs so the runner can settle it. To preview a known subset, use a separate recipe with [exclusions](../docs/guide/datasets.md#exclusions) for the groups to leave out, then select the reporting stage with `--no-upstream`. SPIT checks the collection constraints when it builds that recipe's DAG.

## Scripts as inputs

The runner identifies a job by its command text, and the DAG lists files a job declares as inputs. To make edits to a script invalidate just the operation that reads it, declare the script as a source and pass that source to the operation. A script kept beside the pipeline is declared `here`, which finds it from the pipeline's folder rather than the dataset (see [Files beside the pipeline](../docs/guide/operations.md#scripts-and-settings-as-inputs)):

```spit
source script : Script .py here
source lines : Lines .txt

default path: out/{@product}
path script: scripts/transform.py
path lines: input.txt

operation transform(script: Script, lines: Lines) -> Lines .txt
command transform: python3 {script} {lines} > {@output}

result = transform(script, lines)
```

With `scripts/transform.py` beside `analysis.spit` and `input.txt` under the dataset root, run the pipeline with `spit-bash run analysis.spit --root DATASET`. The compiler puts `scripts/transform.py` in the job's input list. When it changes, that operation reruns; a command argument that is not declared as an input is not inferred as one.

## Tools

A recipe's `tools from` line names a `.spitenv` that says what each tool is on this machine, and the pipeline's `uses` lines say which tools each operation's jobs depend on (see [Tools](../docs/guide/operations.md#tools)). `plan`, `explain` and `run` run each tool's probe once, before planning, from the dataset root and with no shell, and a probe that fails stops them with an error before any job runs: `error: the probe of tool fsl, sh -c ..., exited with status 1: ...`. A tool's identity, its probe's output or the text the `.spitenv` gives, is part of the work of each job that names it, so a job reruns when one of its tools changes, as `since the last run: register: tool fsl changed (12 jobs)` says, and a job that names none is unaffected.

A run records what each probe printed in `tools.json`, beside the state. When a tool's output differs from that record, `plan` and `run` show the lines that changed before anything else, so a probe that prints something that changes on every call, such as a date, is spotted on its first rerun rather than after many:

```text
tool fsl changed since it was last recorded:
  -6.0.7.22
  +6.0.7.23
```

After an upgrade that should not rerun finished work, or when a tool is added to the `.spitenv`, `spit-bash accept-tools dataset.spitin` accepts the tools as they are now for every job whose record differs from the DAG in its tools alone, so the job is current without rerunning. A job whose command or files changed as well is left to rerun. The record still names the run that made the outputs, but not the tools that run used: `explain` then shows the accepted fingerprint, so read the run's log for what it ran with.

## Planning and running

SPIT writes the absolute dataset `root` into a DAG when it knows it. If `root` is `null`, pass `--root /path/to/dataset`. A DAG also names its `output_root`, where the jobs' outputs go: the dataset root, unless the pipeline or recipe names [a folder of its own](https://github.com/eclnz/spit/blob/main/docs/guide/datasets.md#output-root). Each artifact's path is relative to the root it is under: sources, and files a job writes beside a source, are under the dataset root, `here` sources are under the DAG's `pipeline_folder`, and every other output is under the output root. Commands run from the output root, which the runner makes before the first job, and a command names a file under a separate dataset root by its absolute path, so the dataset itself is only read and may be read-only. With `--root`, an output root that is the dataset root moves with it.

Every artifact must stay under its root once symbolic links are followed, so a job never reads or writes outside the folders the DAG names. `spit dag` follows a link wherever it leads, but `plan` and `run` refuse a link that leads out, naming the link, where it leads and the job that uses it:

```text
error: scripts/plot.py is a link to /home/me/project/pipeline/plot.py, outside the dataset root /home/me/project/data.
  Job 3 (plot) reads it as a source.
  spit-bash reads and writes only under its roots, so no link may lead out of one: move or copy what it links to under the root.
```

A script kept beside the pipeline is declared a `here` source, as in [Scripts as inputs](#scripts-as-inputs), or named with `{@here}`, which this section describes below. No job writes under the pipeline's folder, and `plan` refuses a DAG whose job does.

`plan` first says what changed since the last run, in the terms of the data and the pipeline, as `since the last run: +30 recording (take=3); 1 recording changed (digit=3 take=2 voice=lucas); 131 jobs from earlier runs no longer in the DAG (voice=yweweler); trim: command changed (90 jobs)`. A source is new when the last `run` had not seen it, which `run` saves in `sources.json` beside the state before any job starts; a job with no successful record, because it failed, was blocked or was left out of the run, says nothing about its inputs, so they are not called new, and `plan --verbose` and `explain` give that job's own reason. A source is named by its entities, or, among many, by the values no other source has. Work gone is the records no job claims, named by the values only they made. A first run, or one where nothing changed, has no such line. Then `plan` prints counts, such as `13 jobs can start, 68 jobs will wait, 12 jobs are blocked`, then a line for each cause that blocks jobs, such as `blocked: parcellate_t1 x3: program not found: mri_synthseg (9 more depend on them)`, because a cause is what needs action. A job can start when what it reads exists; it waits when a job upstream will, or may, rewrite what it reads, and cannot start until that job finishes. A job below a blocked one is blocked, and counted under the cause. A job that will run over outputs already on disk is told apart from one making its outputs for the first time, since running it replaces them: `plan` gives their count and a line for each operation and reason, such as

```text
4 jobs will replace existing outputs:
  register_asl x2: no successful run recorded
  convert_image x2: input changed
```

Jobs making their outputs for the first time are only counted. `run` prints the same lines before it starts. `plan --verbose` lists every job as `ready`, `waits for N`, `may run`, `place`, `check`, `current` or `blocked`, with its reason. `plan` exits 1 if any job is blocked, with or without `--verbose`. A `may run` job is current as it stands, but a job it reads from will run, so whether it must is not known until that job has finished: see [Stopping at an unchanged output](#stopping-at-an-unchanged-output). A `place` job, under `--move-files`, has its outputs moved rather than made: see [Existing paths and reruns](#existing-paths-and-reruns). `plan --json` gives each job's status (`run`, `maybe`, `place`, `check`, `skip` or `blocked`), its reason in words, a `code` for the rule behind it that stays the same when the words change (such as `output_missing`, `input_changed`, `program_not_found`, `missing_external` or `dependency_blocked`; the full list is `Why` in `src/spit_bash/cause.py`), the jobs it waits for (`waits_for`), the blocked job whose cause is its own (`caused_by`), how many jobs it holds up (`holds_up`), and for a `place` job its `moves`, each `from` and `to`. `run` makes the same plan and prints the same counts; `run --verbose` lists the jobs too. `run` plans first and acts on the plan: while any job is blocked it starts nothing, prints the counts and causes, and exits 1 (`nothing was run: 12 jobs are blocked. Fix the cause, or pass --partial to run the jobs that are not.`), so no tool is called with a missing program or input. `--partial` runs every job that is not current or blocked, leaving the blocked ones, as for a dataset that is still being collected. Otherwise it runs every job that is not current. It runs each job's input checks and `verify` commands in order, then its [commands](#several-commands), then its output checks, and only starts a dependent after its producers finish successfully. Independent jobs run together, as many as the run's cores and memory hold: see [Cores and memory](#cores-and-memory). When more jobs are ready than can start, the one at the head of the longest chain of work still to come starts first: each job is ranked by its own expected time, as in [the estimate](#output-and-logs), plus the longest of the ranks of the jobs that depend on it, a job with no time counting as the median of the times the state holds, or 1 second when it holds none. Jobs of equal rank start in DAG order. So a long chain, such as a subject's preprocessing followed by its model fit, starts early, rather than finishing alone at the end of the run while the other cores sit idle. Commands are passed as argument arrays directly to the operating system; no shell parses them. Output parent directories are created when a job starts. A command must create every declared output file, or [folder](#folders), to count as successful. A declared output is never optional: a job whose commands exit 0 but leave one missing fails, records no success, runs no output checks and holds back its dependents, and the failure names each missing output's port and path, as `command succeeded but did not create output meta derivatives/image/sub=01.json`. A tool that chooses its own file names, as `dcm2niix` adds `_real` or `_e2` to some images, writes beside the output instead, so the failure also names each file that appeared in a missing output's folder while the job ran, other than the DAG's own files: `command succeeded but did not create output image derivatives/image/sub=01.nii.gz; it wrote derivatives/image/sub=01_real.nii.gz in the same folder, left in place`. Those files are not removed with the job's outputs. A tool run with a flag that turns a file off, such as `dcm2niix -b n` and its `.json`, needs an operation that does not declare that file.

`explain ID` explains one job's decision from the plan `plan` makes, as in `spit-bash explain 12 dataset.spitin`: the status and reason, the job's recorded and current fingerprint and checks, the input and output files that differ from the record with their recorded and current sizes and times (a folder's total size, file count and latest time), whether a file whose stamp differs has the same content, different content, or no recorded hash to compare, and the upstream jobs that will or may rewrite its inputs. A job that now reads other artifacts than its record holds, as an aggregate does once a verdict changes its members, has no input file compared with the record. It runs nothing and writes nothing, not even to a state log with a truncated last line. An unknown job id is an error.

```text
Job 1: op1
  decision: run, input changed
  fingerprint: same (recorded 620b9a143dd25648, now 620b9a143dd25648)
  checks: same (recorded none, now none)
  inputs that differ from the record:
    source.txt
      recorded: 5 bytes, mtime_ns 1794032177977430844
      now:      8 bytes, mtime_ns 1794032178028297347
      content differs
```

A command that names a script kept beside the pipeline with `{@here}` gets the pipeline's absolute folder from the DAG's `pipeline_folder`, so such a DAG runs on the machine whose folder it names; run it elsewhere by planning from the recipe there. A blocked job blocks the jobs that depend on it. `run` starts nothing while any job is blocked, as above; with `--partial` it runs every other job, then exits 1. A job is blocked when an external input is missing, when the program its command, a `verify` command or a check starts with cannot be found, when a `{@here}` path in one of them does not exist under the pipeline's folder, or when a job it depends on is blocked. That reason is `here_not_found` in `plan --json` and reads `script not found: PATH`; a `{@here}` that names a folder needs only the folder, and a script is still not an input, so editing one reruns nothing. Programs are looked up on `PATH`, from SPIT's `executables` list; a program named by a path, such as `bin/check`, is looked up from the output root, where commands run.

## Folders

A DAG marks each artifact as a `file` or a `folder`, for a product SPIT declares with a `/` after its type, such as `source dicom : Dicom / [sub]`. The runner treats a folder as one artifact:

- A source folder must exist as a folder, or the jobs that read it are blocked.
- Before a job's first command runs, and after its `verify` commands pass, the runner moves whatever an earlier run left at each of its output folders [aside](#when-a-job-fails). The job owns the folder, since SPIT puts no other artifact inside one, and old files would otherwise mix with the new run's. Then it makes the folder again, empty, for the tool to write into, as tools given a folder to fill, such as `bwa index -p` or `dcm2niix -o`, expect. Copy a folder's contents into it with `cp -R {notes}/. {@output}`: `cp -R {notes} {@output}` puts a copy of `notes` inside the folder. A tool that refuses to write into a folder that exists is wrapped in a command that removes it first.
- After the last command, each output folder must exist. It may be empty. If the job fails once its first command has started, the folder is [removed](#when-a-job-fails) with everything in it.
- An output folder declared `/ from input` starts as a copy of that input folder rather than empty, made afresh for each attempt, so the command changes its own copy and never the input. A relative link whose target stays inside the input folder is copied as a link; any other link, or anything that is neither a file nor a folder, fails the job before its first command: `could not start featured/sub-02 as a copy of its input folder: bad in ... is a link to /etc/passwd, outside the folder`.
- A directory view, an output declared `/ view`, is no job of its own: it names a folder that other jobs' files are in. A job that reads it gets the folder's path, waits for every file the view lists, and reruns when one of them changes; files in the folder that the view does not list are not tracked. The names of the listed files are part of the reader's work.
- The runner compares a folder by the files under it, at any depth, not by the folder's own modification time: [Existing paths and reruns](#existing-paths-and-reruns) says how.

## Checks

A DAG gives each job its `checks`: commands that each test one artifact, which the pipeline declares once with SPIT's `check` and attaches to ports and sources. A `before` check tests an input before the job's `verify` commands; an `after` check tests an output once the command has made it. A failed check fails the job, even when its command exited 0, and the jobs that depend on it do not run:

```text
[2] failed: check contains(today) on output output output/copied/day=tuesday.txt failed: exited with status 1
```

A successful run records the checks the job passed. The runner leaves checks out of a job's fingerprint, so a changed or new check does not rerun the job: `plan` marks it `check`, and `run` runs its checks alone on the files it already has. If they pass, the record takes the new checks; if one fails, the job fails and its record is dropped, so the next run reruns the job. A job with no command whose outputs exist is checked on every run. A dependent that runs or is checked in the same run waits for those checks, and fails if they do; a current dependent stays current. `adopt` records no checks, so the next `run` checks the adopted files.

## Where jobs come from

In SPIT, an operation can be carried out by a body of steps, often declared in a library, and a call to it becomes ordinary jobs. The DAG says where each such job came from: the files the pipeline was read from, each call, and the line of the body's step. When one of these jobs fails, `run` prints the step and every call it is nested in, innermost first:

```text
[1] failed: command exited with status 3
[1] step at libs/lib.spit line 10
[1] in `m::cleaned = L::tidy(...)` at libs/lib.spit line 13
[1] in `m = L::summarise(...)` at main.spit line 8
```

None of this is part of a job's fingerprint, so an edit to a library that leaves a job's command as it was reruns nothing.

## Several commands

A job's `commands` in the DAG run in order, each once the one before it exits 0. A SPIT command written as a block runs a line each, as an aligner's output is sorted and then indexed in one job:

```spit
command sort:
    samtools sort --threads {@cores} -o {bam} {sam}
    samtools index {bam}
```

- Each command runs as a job's only command does: in a process group and session of its own, stopped with the job when the run is, held to the job's memory, with the standard input and output described [below](#standard-input-and-output).
- The first command that fails stops the job, and the commands after it do not run. The failure names it by its place, from 1, as `command 2 exited with status 1`; a job of one command says `command`, as `command exited with status 1`.
- Once the first command has started, a failure in any of them [removes the job's outputs](#when-a-job-fails), what an earlier command wrote included, and `--keep-failed` keeps them.
- The log shows each command after `$ `, in the order they ran:

```text
$ samtools sort --threads 4 -o out/bam/sample=A.bam out/sam/sample=A.sam
$ samtools index out/bam/sample=A.bam
```

## Standard input and output

A SPIT command written `wc -l < {table} > {@output}` reads an input file on its standard input and writes an output file on its standard output. The DAG gives the command as `wc -l` alone, and names the two files in its `stdin` and `stdout`:

- The runner opens the `stdin` file for reading as the command's standard input. A command without one reads nothing: its standard input is `/dev/null`.
- The runner writes the command's standard output to a temporary file beside the `stdout` output, named like `.x.txt.spit-bash-partial` for `x.txt`, and renames it onto the output only once the command exits 0, before the next command runs, which may read it. So a failed or stopped command never leaves a partial output at its path, and the temporary file is removed with the job's other outputs. After the rename the output is like any other: it must exist, which an empty file does, and its output checks run on it.
- The command's standard error goes to the job's log in either case. A command without `stdout` has its standard output logged as well.

The log shows the redirections on the command's line, as a shell would write them:

```text
$ tr a-z A-Z < input/allotment.txt > output/loud/plot=allotment.txt
```

`verify` commands and checks never redirect. When the DAG is loaded, each command's `stdin` must be one of the job's input files and its `stdout` one of its output files, neither a folder, and one of a job's commands at most has a `stdout`; a DAG that breaks one of these is rejected.

## Numbers read from files

A SPIT command written `oxford_asl --tr @scalar(tr, min=5) ...` passes the number in the input file `tr` as an argument. Once a job's input checks pass, before its `verify` commands, the runner reads each such file and writes what it holds to the job's log:

```text
spit-bash: derivatives/tr/sub=01.txt holds 6.234000
$ oxford_asl --tr 6.234000 ...
```

- The file must hold one number written as a decimal, such as `6.234000`, `-2` or `1e-3`, with only whitespace around it. The argument is the number as the file writes it.
- A file that holds anything else, or a number outside the bounds, fails the job before any of its commands start, as ``derivatives/tr/sub=02.txt, read by `@scalar`, holds 4.1, which is under the min of 5``.
- The file is one of the job's inputs, so the job reruns when it changes. `plan` and `explain` show the read, as `@scalar(derivatives/tr/sub=01.txt, min=5)`, since the number is known only when the job starts.

## Files that list a job's inputs

A SPIT command written `register_tool --file-list @list(images) ...` passes the path of a file listing the `many` input `images`, one absolute path to a line. The runner writes the file in a folder of its own under the system's temporary folder once the job's input checks pass, before its `verify` commands, gives the same file to each of the job's commands, and removes it when the job ends, whether it succeeds, fails or is cancelled. A file that cannot be written fails the job before its commands start. `plan` and `explain` show the part as `@list(...)` with its paths, and where the file is written is not part of the job's work, so it does not make a job rerun.

`@join(images)` needs nothing at run time: the runner joins the paths as commands name them, and refuses a DAG in which a path holds the separator.

## Reviews

The runner reads the current review CSVs at the start of each invocation and reports rejections and artifacts ready to inspect. A saved DAG works across review rounds. Work held for a missing verdict or excluded by a rejection leaves the exit status unchanged, and the run's summary counts it, with where to record verdicts on the next line:

```text
4 done, 0 checked, 0 failed, 2 held for review, 1 not required, 0 current, 0 blocked
review `looked_at`: 1 rejected, 1 held
```

If a verdict changes an aggregate's membership, the runner reevaluates its work using the surviving members.

At the end of a run, the runner creates any missing configured review CSV that has artifacts ready to inspect. Open the path printed in the review summary, use `@evidence` to find the file to inspect, and enter `pass` or `fail` in `verdict`. Add a `reason` if useful. An empty verdict stays pending. Generated sheets use `@artifact` to identify each review case and `@revision` to identify the reviewed contents. When the candidate or its evidence changes, the runner holds downstream work and adds a blank verdict row after the new files are ready. Enter the new verdict on the last row for that artifact. The sheet preserves earlier verdicts and reasons. Hand-written dimension-based CSVs supply standing verdicts across revisions. `plan` and runs using `--bypass-verdicts` leave verdict CSVs alone.

```sh
spit-bash run study.spitdag --bypass-verdicts
spit-bash run study.spitdag
```

The first invocation produces potential outputs through the review and screen gates. Artifact checks still run. The second applies the current verdicts and reuses matching work. Outputs excluded by a verdict remain on disk.

Local runs evaluate screens after checks and record the outcomes in product `.screens.csv` files. Removing a record makes the next invocation screen the product again. Runs using Slurm support human review gates and `--bypass-verdicts`. If a selected screen lacks a human override, run with the local executor to evaluate it.

## Cores and memory

A job's `cores` and `memory` in the DAG come from its operation's `resources` line, as in `resources merge: cores=2, memory=64M`: a job needs one core and claims no memory when its operation declares neither. `run` has a number of cores and an amount of memory to share among the jobs it runs at once:

```sh
spit-bash run dataset.spitin -j 16 --memory 64G
```

- `-j CORES`, or `--cores CORES`, sets the local core budget (default 4). A pipeline that declares no `resources` can run up to that many jobs at once.
- `--memory SIZE` is the memory to use, a whole number with a unit `K`, `M`, `G` or `T`, each 1024 times the one before, as in a `resources` line, or a number of bytes. By default it is the machine's physical memory; where the system does not report it, memory sets no limit.
- A job starts only when its cores and memory are free beside those of the jobs already running, and holds them until its output checks finish. A job with no memory claim needs none.
- A job's declared memory is a limit. While each of its `verify` commands and commands runs, `run` reads twice a second how much memory the command's process group holds, from `/proc` on Linux and `ps` elsewhere, and kills the group at once when it holds more than the job declares. The job fails, as `command used 212M of memory, more than the 64M its job declares, and was killed`, and loses its outputs as any failed job does. Raise the job's `resources` line and run again. A job with no memory claim is not limited, and checks are not. The reading counts each process's resident memory, so pages that forked workers share count once for each worker, and a group of them can read higher than it holds. The reading is taken every half second, so a command that goes over its declaration only between two readings, or ends before the first, is not caught: a limit catches a job that grows, not a brief spike.
- A job that declares more memory than `--memory` is refused before anything runs, and `run` exits 2 naming it, as `[3] align needs 64G of memory where the run has 32G`.
- Ready jobs start in rank order and no other. While the job at the head waits for cores or memory, the jobs behind it wait too, even ones that would fit: the head is on the longest chain of work, and letting smaller jobs past it could delay the run or keep a big job waiting for good. Some cores may sit idle while a big job waits for enough to free.
- A job that needs more cores than `-j` takes all of the run's cores and memory: it starts once no other job is running, and runs alone. `run` says so as it starts, as `[3] needs 16 cores where the run has 8; it will run alone, once no other job is running`.
- `--no-enforce-memory` turns both memory rules off: a job may hold more than it declares, and one that declares more than `--memory` runs alone, as a job needing more cores does.
- A job planned `check` runs only its checks, but waits for the job's cores and memory as a run of it does.

## Running on a Slurm cluster

`--executor slurm` runs each job as a task of a Slurm job array, while spit-bash, the controller, plans, watches the queue and keeps the state. It runs on a login node, or as a long single-core job; the dataset, the output root and the runner's folder must be on a filesystem every node shares, and spit-bash installed where every node finds it.

```sh
spit-bash run dataset.spitin --executor slurm --profile cluster.toml
spit-bash plan dataset.spitin --executor slurm --profile cluster.toml
```

A profile is a TOML file of what is specific to a cluster or a project, so the pipeline stays the same on a laptop and on a cluster. Every key is optional:

```toml
account = "lab"
partition = "compute"
time = "10m"                # --time for a job with no recorded time (default 10m)
memory = "2G"               # --mem for a job whose operation declares no memory
options = ["--qos=normal"]  # more sbatch options
setup = ["module load fsl/6.0.7"]

jobs = 100                  # at most this many live or unresolved tasks per runner folder
submit_rate = "50/1m"       # at most this many sbatch calls in this time
array_limit = 20            # --array=...%20: tasks of one array running at once
status_interval = "2m"      # how often squeue is called, once for the whole run, while tasks change
max_status_interval = "16m" # the longest gap, which doubles from status_interval while nothing changes
result_interval = "2s"      # how often results are read and ready jobs submitted
visibility = "1m"           # how late the shared filesystem may show one node's writes on another
time_factor = 3             # a job's --time is its last run's time times this,
min_time = "5m"             #   within these bounds
max_time = "2d"
retries = 2                 # a job Slurm stopped for memory or time runs again (default 2),
memory_factor = 2           #   with this much more memory,
retry_time_factor = 2       #   or this much more time,
max_memory = "64G"          #   but never more than this, or than max_time
short_job = "1m"            # name operations whose jobs average less than this
local = ["read_header"]     # operations the controller runs itself
local_cores = 1
require_site_check = true   # a run needs a passing `check-site` for this profile (see below)

[operations.register]       # an operation's jobs, and
memory = "16G"
time = "6h"

[stages."preprocess"]       # a stage's, with the stages inside it
partition = "short"
```

### Checking the cluster first

`spit-bash check-site` tests what a run assumes about the cluster, with a few tiny jobs, before a run trusts it:

```sh
spit-bash check-site dataset.spitin --profile cluster.toml
spit-bash check-site --dir /shared/project/.spit-bash --profile cluster.toml --dry   # what it would do
spit-bash check-site dataset.spitin --profile cluster.toml --enforcement             # also end jobs on purpose
```

```text
pass  slurm commands      sbatch, squeue, scancel and scontrol are on PATH
pass  profile             Slurm accepts the profile's request, by `sbatch --test-only`
pass  limits              MinJobAge 300s, MaxArraySize 1001, KillWait 30s; a run reads the queue every 120s while its tasks change, and at most every 150s while they do not
pass  queue               `squeue` listed each task with its index and script path. Cancelling one element left its sibling pending. The held array was no longer listed as live after name cancellation.
pass  probe               2 probe tasks ran, on c1, c2
pass  tools               every probe node started spit-bash 0.7.0 as hpc, through the profile's setup lines and command
pass  exclusive create    an exclusive create is decided by the file server, between the controller and each node, and between nodes
fail  visibility          a file took 27.9s to show on another machine, and the profile's visibility is 20s; set visibility to at least 57s
pass  accounting          sacct answers on the controller's machine and on every probe node
pass  stop signal         SIGTERM reached a cancelled task, and Slurm waits 30s (KillWait) before it kills
site check: 9 passed, 0 warnings, 1 failed
```

Name the runner's folder with `--dir`, or give the DAG or recipe that defines it. The check uses a uniquely named held array and a three-task probe on the runner's shared filesystem. `sbatch --test-only` checks each distinct profile request without queuing jobs. The check has a `--timeout` deadline of 300 seconds by default. On expiry or interruption, it stops the check, requests cleanup of its probes, and records what is still unconfirmed. Cleanup has its own short command timeouts.

What it checks, each a claim a run makes:

- **The shared filesystem.** `exclusive create`: an exclusive create is decided by the file server, between the controller and a node and between nodes, so claims keep one task per job. `visibility`: a file shows on another machine sooner than the profile's `visibility`, measured in both directions with the file looked for before it was written, as a run looks for a result.
- **The nodes.** `tools`: each node starts this spit-bash, DAG version and user, through the profile's setup lines and command. `accounting`: `sacct` answers on the controller and on each node.
- **Slurm.** `profile` tests the requests in the profile. `queue` checks that `squeue -r -t all` reports task indices and script paths, and that cancelling a held element leaves its sibling pending. A name-scoped cancellation cleans up the held probe. `limits` reports `MinJobAge`, `MaxArraySize`, and `KillWait`. `stop signal` checks that SIGTERM reaches a cancelled probe.
- **With `--enforcement`**, a job that exceeds its memory request and one that exceeds its time limit should end as `OUT_OF_MEMORY` and `TIMEOUT`, the states the runner uses for retries. A dependent job reports their accounting states.

A failed or incomplete visibility measurement, rejected profile request, failed queue check, or probe that exceeds the deadline gives a nonzero exit. Accounting unavailability, clock skew, and a short `KillWait` can give warnings. A pass records the observations from this check. The report says when both compare tasks ran on the same node.

The latest report is in `.spit-bash/site/site.json`. Each profile also keeps its own report under `.spit-bash/site/profiles/`. A failed report for the selected profile blocks `run --executor slurm`. With `require_site_check = true`, the profile needs a passing report from the current set of checks before it can submit.

A job's cores and memory come from its operation's `resources` line, as in a local run: `--cpus-per-task` is its cores, and `--mem` its memory, or the profile's `memory`. A key under `[stages."NAME"]` applies to the stage and the stages inside it, and one under `[operations.NAME]` overrides both. A wrong key or value stops the run before planning, as `error: profile: operations.align has unknown key 'cores'; it takes account, memory, options, partition, setup, time`.

Connect to the Slurm cluster where the run should execute. The runner rejects remote routing through profile options or cluster-selection environment variables because queue and accounting evidence must come from the same cluster. Extra `options` use long Slurm options such as `--qos=normal`. SPIT owns the options that control array size, job identity, routing, and requeue.

- **Array admission.** Jobs with the same operation and resources can share an array. Before `sbatch`, the controller writes the array manifest to `.spit-bash/slurm/intents/` and reserves every element in a `.dispatch` file under `.spit-bash/slurm/`. Pending, running, completing, and unresolved elements use slots under `jobs`.
- **Queue evidence.** One startup query discovers the folder's jobs. Subsequent queries can target the admitted IDs. The queue's script path and array index identify an owned job. A matching name alone gives no cancellation authority. A missing task remains charged until Slurm confirms its terminal state. If accounting provides that evidence, its submission comment must match the intent.
- **Claims and results.** An exclusive create lets only one worker hold an intent's claim at an array index. The controller releases it after proof that the exact task ended. A result file can arrive before Slurm releases the allocation. The controller waits for terminal evidence before advancing the workflow and keeps a success result recoverable until the job's state record is written.
- **One visibility bound.** A shared filesystem shows a file one node wrote on another within `visibility`, which must cover every cache between them: NFS's attribute cache alone keeps an entry up to a minute by default, and a cache on top of it adds its own time. Too short a bound costs work, never a wrong result: a job whose input never showed as recorded fails, naming it, rather than read it. A task waits until its node sees each input as the job that made it recorded it, for at most `visibility` after the input was written, and fails naming it if it never does. The controller waits that long for a result of a task Slurm says completed. And a plan made just after a Slurm run waits that long for an output its record says exists, so it judges what the jobs wrote, not what this machine's cache shows.
- **Time limits and retries.** The DAG gives no time per job. A job asks for its last recorded time times `time_factor`, within `min_time` and `max_time`, or for `time` when it has none, so short jobs ask for short limits and start sooner in a busy queue. At the limit Slurm sends SIGTERM, on which the job puts back its earlier outputs, then SIGKILL, and calls the task TIMEOUT. With `retries`, a task Slurm calls `OUT_OF_MEMORY` is submitted again with `memory_factor` times the memory, and one it calls `TIMEOUT` with `retry_time_factor` times the time, never more than `max_memory` or `max_time`. `run` prints `[3] retry   align sub=01: Slurm ended it out of memory; attempt 2 with 4G of memory and 120 minutes`, and each attempt after the first has a log of its own beside the job's. A job's attempt and resources are kept in `.spit-bash/slurm/attempts.json` until it succeeds, so retries stay bounded across runs. When it succeeds, its state record keeps the attempt, memory and time it ran with, `explain` shows them, and the next run of the same work starts at that memory when it is more than the job would otherwise ask for, so it is not killed again before its retry. An unreadable or invalid retry history stops the run with `cannot verify Slurm retry history` and the file’s path. A job that fails of its own accord is not retried.
- **Stopping and restarting.** Ctrl-C and SIGTERM request cancellation for verified task IDs and report that terminal state is unconfirmed. A task Slurm no longer knows counts as ended, so one such task does not hide the rest; for a task `scancel` refuses, the message names it with `scancel`'s own reason. The runner then reads the queue and lists the tasks still in it with their states, with the `scancel` command for them. Each task `scancel` accepted is kept in its reservation as cancelled, so a later run treats one the queue no longer lists as ended, though accounting has no row for a task cancelled before it started. A restart adopts identifiable tasks and confirms cancellation of obsolete work before admitting replacements. An unanswered `sbatch` retains its reservation across restarts. A reservation the runner cannot settle, because the queue no longer lists a task and accounting shows no end for it, or because acceptance was never seen, stops a run with the reason and a `spit-bash release --dir DIR` command. `release` lists those reservations, and with `--yes` releases them on your word that none of their tasks is running unseen, deleting each one's batch so a task that starts later writes nothing. It refuses while `squeue` lists any task of the folder, printing the `scancel` for them, and never offers a task whose `sacct` did not answer. The release is recorded as a run that `ended: released`.
- **One controller.** The controller acquires `.spit-bash/slurm/controller.json` with exclusive creation and checks it before changing scheduler jobs. On the same host, a new controller can recover a lease whose process has exited. For a lease on another host, verify the old process has stopped before removing the lease.
- **Setup lines.** `setup` lines run before `spit-bash job` in each task. Before planning, the controller runs every set of setup lines once in a login shell and adds the `PATH` they leave to its own, so a program that appears only after a `module load` is not reported missing. A program one operation's lines provide counts as found for every operation, so a job whose own lines lack it fails when it runs.
- **Local operations and short jobs.** Jobs of the operations in `local` run on the controller, on `local_cores` cores, as a local run would run them. Starting a Slurm job costs the scheduler more than a job of a few seconds takes to run, so after a run each operation with five or more Slurm jobs that took under `short_job` on average is named, as `note: 120 jobs of read_header took 2.3s each on average as Slurm jobs; ... Name read_header under `local` in the profile to run its jobs here instead.`
- **One state.** Only the controller writes the state. Local and Slurm runs share it: a dataset can move between them, and `plan`, `restore`, `clean` and the run history give the same answers whichever ran its jobs.

Being a good neighbour on a shared cluster:

- **Scheduler budgets.** The default is 100 live or unresolved tasks per runner folder and 50 submission calls per minute. The submission allowance persists across restarts. Quota refusals back off and retry a bounded number of times, retiring the refused intent files.
- **Queue reads.** The queue is read once for the whole run, every `status_interval` while tasks change. While a read finds nothing new, the gap doubles, up to `max_status_interval`. The target gaps are capped at half the site's `MinJobAge` to leave time to see finished tasks. `check-site` reports the effective gaps. A delayed read can miss a finished task, in which case the runner looks for matching terminal evidence in accounting. If the queue cannot be read, the controller stops submitting and keeps its unresolved reservations. The runner appends Slurm call counts to `.spit-bash/slurm/call_logs.jsonl` on the first call, on subsequent calls at least a minute after the last write, and on exit. Each line has cumulative counts for the invocation, identified by its start time and PID, and any new timeline entries. A line with `"final": true` marks cleanup.
- **Ask for what jobs use.** Give each operation a `resources` line near what it needs: Slurm reserves what a job asks for, whether it uses it or not.
- **Keep the controller light.** Jobs hash their own files on their nodes, but planning hashes on the controller any file whose size or time changed since it was recorded. When a plan would hash much, as after copying a dataset, run the controller as a long single-core job rather than on a login node.

`tests/cluster` holds a Slurm cluster in Docker, with nodes that see their shared folder late, for trying this out: see its [README](tests/cluster/README.md). `tests/slurm-checks.sh` runs the long checks, which do not run on GitHub, with [uv](https://docs.astral.sh/uv/) providing the runner and its test dependencies, as under [Development](#development). The test dependencies include Hypothesis, which generates scheduler failure sequences against the runner.

## When a job fails

For a Slurm job, a verified cause such as `OUT_OF_MEMORY` appears on the failure line alongside any command or check failure the worker reported.

A command writes its outputs where they belong, so one that fails can leave a truncated file that another tool, a person or `spit discover` would take for a finished one. Once a job's first command has been started, any failure removes everything at the job's declared outputs, files and folders alike: a non-zero exit, a command that could not start, a missing output, an input that changed while it ran, a failed output check, or a run that was stopped. An output that passed the command but failed a check is complete but rejected, and is removed too. The runner removes them only after the failed command's process group has stopped, so nothing is still writing, and the job's log ends with a line for each output it removed:

```text
spit-bash: removed failed output work/upper.txt
```

Before its first command starts, a job moves what an earlier run left at its outputs aside, to a name such as `work/.upper.txt.spit-bash-previous`, so that it starts from the state a first run does and a tool that refuses to write over a file, such as `gdalwarp`, works on a rerun. When the job succeeds, the earlier version is [kept in the run's folder](#run-history), or deleted with `run --no-keep-replaced`, unless the job reran because that output was edited since the job made it: then the edited version is kept beside the new one as `NAME.edited-<time>`, with the time the run started, and `plan` and `run` list each such job first, as `[880] edited: out/by_digit/digit=3.tsv`. When it fails, or the run is stopped, the failed output is removed as above and the earlier version is put back, with a line in the log: `spit-bash: restored the earlier work/upper.txt`. The job's record is kept until the job succeeds, so the record and the files put back still agree: a job that was current before a failed rerun is current again once what made it rerun is undone. With `--keep-failed`, the failed output stays, the earlier version is deleted, and so is the job's record.

A job that fails at a missing input, an input check or a `verify` command keeps its earlier outputs and record. A job that fails while preparing its output paths also puts back any outputs it moved aside, including with `--keep-failed`. For example, if a file named `work` appears after planning and blocks creation of the parent directory for `work/upper.txt`, the failure reports that it could not prepare or check the job's files and names the blocked path. To debug a tool, `run --keep-failed` leaves what a failed job's commands wrote where it is, and the log says `spit-bash: kept failed output work/upper.txt` instead. An output the command writes on its standard output stays in its temporary file, such as `work/.upper.txt.spit-bash-partial`, and is never renamed onto the output. The job's record is dropped, so a kept output is never taken for a finished one: the next run runs the job again. The log also holds what the command printed, other than an output it writes on its standard output, and each command it ran, after `$ `, to run again by hand.

## Output and logs

Each job's output, from its checks, its `verify` commands and its commands, goes to its log in the run's folder, `.spit-bash/runs/RUN/jobs/PRODUCT/ENTITIES.log` under the output root, named by the job's first output, as `jobs/clean/voice=theo_digit=7.log` (see [Run history](#run-history)), except a command's standard output when it is [one of the job's outputs](#standard-input-and-output), which each run of the job replaces. The terminal shows only when each job starts, finishes or fails, so parallel jobs do not mix their output. When a job fails, `run` prints the last lines of its log. A job's log starts with why the plan ran it, as `spit-bash: planned to run: input changed`, so the log still answers that once the job's record has been replaced.

`--timestamp-logs` starts every line of a job's log with the date, time and UTC offset at which the runner received it, to the millisecond, such as `2026-11-07T15:12:03.142+13:00 $ bet ...`. The runner's own `$` command lines, its notes, and every line of a command's output are stamped; a command's output written to a declared artifact is not touched. A tool that buffers its output may have written a line earlier than its stamp, and lines that arrive in one read share a time. A last line with no newline is ended with one. Without the flag, a log holds the lines as the command wrote them.

```sh
spit-bash run pipeline.spit --timestamp-logs
```

`run` prints the plan's counts, then a blank line and `Running`, with how many jobs will run and how long it expects to take, then a line as each job starts and finishes, how long it took and how many jobs are left, and a line as a job fails, with its error, so a run can be stopped as soon as a failure shows it needs fixing. A job is named by its operation and by what it works on: the entities of its first output, such as `sub=02 task=rest run=01`, in name order, or the group a group job makes. Where those leave two jobs of one operation looking alike, as for jobs with no entities, the product is shown too, and a long value is cut short with `…`. The job id stays the reference to a failure.

```text
12 jobs can start, 40 jobs will wait for them

Running 12 jobs, about 4s
  [ 1] start   anat sub=01
  [ 4] start   motion run=01 sub=01 task=rest
  [ 1] done    anat sub=01 in 2s; 11 left, about 2s
  [ 4] failed  motion run=01 sub=01 task=rest: command exited with status 1
```

Once the run ends, a failed job's error and the end of its log are printed once. Jobs that failed the same way, with the same last lines once the paths of their own files are left out, are printed once and named: `[1] the same failure in 2 other jobs: 2, 3`. Each failure says how many jobs it held up. A job that did not run because a job it needs failed is not a failure of its own: it is counted as `not run`, and `run --verbose` names it, as `[2] not run: job 1 failed`. The summary counts them apart, as `0 done, 0 checked, 13 failed, 68 not run (an upstream job failed), 0 current, 12 blocked`. No tool is started for a job below a failed or blocked one.

A job's `done` line comes once the runner records its success.

A job is expected to take as long as its last successful run did, which the state records. A job that has not run before is expected to take the mean time of its operation's other jobs, from earlier runs and from this one. Until every job left has an estimate, `run` gives the count alone. A `maybe` job counts as one that runs until it is skipped, so the estimate is the longest the run can take. The estimate is the longer of two times the run cannot beat: the work left, each job's time multiplied by its cores, divided among the `-j` cores, and the longest chain of jobs left that each wait for the one before, such as a group template made after every subject and then a statistic made from it, which more cores do not shorten. A running job counts for the time it has left: its own time less how long it has run, or, once it has outlasted its own time or when it has none, the mean of its operation's times that are longer than it has run, less that time. When none of its operation's times is that long, as for the last jobs of a run whose jobs all started together, `run` gives the count alone. The longest chain is worked out again at most every 5 seconds, as times come in. Neither allows for how the jobs will actually fit together on the cores, nor for jobs waiting for memory, so the run may take longer than either.

## Run history

`spit-bash status DAG` (or `--dir DIR`, when the pipeline does not compile) says what the runner's folder holds: Slurm reservations and the site check, the state, the last run in the words of its `summary.json`, kept copies, and, given a DAG, leftovers. It reads and changes nothing, makes no Slurm call when there are no reservations, and exits 1 only when something needs you: a reservation it cannot verify, a failed site check, an unreadable state or outputs with no state, a last run that did not finish, failed jobs, or work held for review. Kept copies and leftovers are listed with the commands that act on them, and need no attention.

Each `run` gets a folder of its own, `.spit-bash/runs/RUN`, named by when it started, as `2026-11-12T10-57-00`, with `-2` after a second run within the same second. It holds:

- `run.log`, what the run printed, written line by line as it goes, so `tail -f` follows it and a stopped run keeps everything up to that point;
- `plan.json`, the plan the run acted on, as `plan --json` prints it;
- `summary.json`, how the run ended, `finished`, `blocked`, `over memory`, `declined`, `interrupted` or `terminated`, with its counts; a run killed before it got there has none;
- `jobs/`, each job's log, named by the job's first output and its entities, so a job's logs share one name from run to run whatever its id, and a rerun writes a new log rather than over an old one;
- `replaced/`, the outputs the run's jobs replaced, each job's under the same name with the record that described them.

Each record in the state names the run that made its outputs, so `explain` shows the log behind a current job, as `made by run 2026-11-12T10-57-00; log at .spit-bash/runs/2026-11-12T10-57-00/jobs/final.txt.log`. `spit-bash log` lists each chosen job's logs across runs, newest first, and marks the one that made its current outputs:

```text
$ spit-bash log pipeline.spitin --product final.txt
[2] op2
  2026-11-12T11-02-08  .spit-bash/runs/2026-11-12T11-02-08/jobs/final.txt.log  (made its current outputs)
  2026-11-12T11-02-06  .spit-bash/runs/2026-11-12T11-02-06/jobs/final.txt.log
```

A run that replaced outputs says where it kept them, and how much disk they take, with the command that puts them back. `spit-bash restore --dir DIR --run RUN` puts back what that run replaced, with the records they had, reading only the runner's folder `DIR`. Give the DAG or recipe in place of `--dir` to name the folder through it, and add `--entity`, `--product` or `--stage` to restore only the jobs they choose, which needs the DAG. A restored record describes the restored files, so the next plan reruns what reads them, as for any other change. What is at those paths when restoring is kept in turn, in a new run folder, so a restore is undone the same way, and a run's kept outputs are restored once. Kept outputs take as much disk again as the outputs they replaced; `run --no-keep-replaced` deletes them instead, for a pipeline whose outputs are too big to keep twice.

`spit-bash prune --dir DIR --keep-runs N`, or with the DAG or recipe in place of `--dir`, removes all but the newest `N` run folders, but keeps, in an older run's folder, the log of each job whose current outputs that run made, so the log behind every current output stays. Outputs a pruned run kept are deleted with it. Nothing is pruned without being asked. A `.spit-bash/logs` folder from an earlier spit-bash is no longer written or read, and can be deleted.

## Stopping at an unchanged output

A job that reads the output of a job that will run is `maybe` when its own record is current: the same fingerprint, files and checks as the last successful run. `plan` says how many jobs can start, how many will wait, and how many more may run. `run` waits for the jobs a `maybe` job reads from, then compares the files it now reads with its record, by size and time and, where those differ, by content. If the producer wrote the same bytes, the job is skipped, its new stamps are stored, and its own dependents are settled the same way. If they differ, it runs. The summary counts the skipped ones apart from the jobs that were current all along, as `1 done, 0 checked, 0 failed, 0 current, 2 kept (upstream reran with unchanged output), 0 blocked`.

A job with a reason of its own, such as a changed input, a missing output or a changed fingerprint, is `run`, not `maybe`. So is a job whose input does not exist yet, since there is nothing to compare. `--compare stat` turns the early stop off, as it has no hashes to compare: every job below a rerun is `run`. A job with no command that reads a `maybe` job is blocked, as it is when it reads a `run` job. A `maybe` job whose program is missing is blocked, as a `run` job is, even if it would be skipped.

## Stopping a run

Ctrl-C (SIGINT) or SIGTERM, as a cluster scheduler sends at a time limit, stops every running job's process group and exits with 130 or 143. Each job's group gets SIGTERM, then SIGKILL once the command has exited or five seconds have passed, whichever is first, so a tool that ignores SIGTERM or hangs while cleaning up cannot keep the run from stopping. Each job runs in a session of its own, so a tool's own child processes are stopped with it. What a stopped job's command had written at its outputs is [removed](#when-a-job-fails), and the earlier run's outputs are put back, with their record kept, so the next run plans the job as this one did. If spit-bash itself is killed mid-job, what is at the job's outputs no longer matches its record, so the next run runs it again.

## Existing paths and reruns

The runner records successful jobs in `.spit-bash/state.jsonl` under the output root, keyed by the products and entities they make, not their paths, so a dataset keeps its state when it moves or its path rules change. The runner computes each job's fingerprint from the shape of what it reads and makes and its commands, with every path replaced by its place in the job, so renaming an operation, a port, a product, a dimension, a source or an entity value changes nothing; SPIT writes none. What the inputs hold is checked against the record's hashes. A job with no record under its key takes an unclaimed record of the same work when each input holds what that record's did, so a renamed product, dimension or source reuses its earlier outputs (with `--move-files`, they are moved to their new paths). Identical inputs, such as copies of one mock file, are interchangeable, so a job first tries the records that keep its dimensions and values, as a renamed product's do, then those that keep its values, as a renamed dimension's do: each job takes its own record, and moves its own outputs. Stamps and hashes in a record are kept by place in the job as well, so files moved or copied to new paths are current when their size and time, or their content, match the record; files that are not at the new paths run the job, and when the pipeline's path rules change, a job whose files are not at its new paths runs again, as its output is missing. Files already at the new paths are reused with or without `--move-files`, when they match the record. The state is version 9. The runner reads the versions it has released (8 and 9), and the first write of a run brings an earlier file to version 9 with its records kept. A newer state is refused with a message to upgrade spit-bash. One that cannot be read is refused with a message to move it aside, which starts from fresh records and runs every job again. The record keeps the fingerprint's parts, so a changed job is reported as `command changed`, `verify commands changed`, `the artifacts it reads changed` or `the artifacts it makes changed`, and a missing output as `output missing; an earlier run left it at PATH` when a change of path rules stranded it. A job runs if an output is missing, its fingerprint changed, an input or output file changed, or a producer will run. It skips only when its recorded fingerprint still matches and its files are as recorded: the same size and modification time, or, where those differ, the same content. A `touch`, a `git checkout` or a copy that leaves the bytes alone does not rerun a job. `--force` reruns every command job, or only the jobs matched by `--entity`, `--product` or `--stage` when a selection is given; see [Choosing jobs](#choosing-jobs). A job that reruns its recorded work on the inputs its record holds, as under `--force`, and makes outputs whose content differs from the record's, has a tool that doesn't give the same bytes twice, such as one that dithers or stamps the time. `run` then notes it once per operation, as `note: normalise gave different output from the same inputs (8 jobs); what reads it will rerun every time`, since nothing below it can stop at an unchanged output.

At a terminal, `run` asks before it replaces the outputs of a job whose last recorded run took at least `--ask-over` (default `2m`; `90s`, `1h` and the like are taken), listing those jobs by operation with the time their outputs took, after the plan's own list of why each job replaces outputs. It starts nothing, deletes nothing and exits with status 1 unless the answer is `y`. A job planned to run only if an upstream output changes, and a job run under `--force`, are not asked about, and neither is anything with `--yes`, or when standard input is not a terminal or holds the DAG.

```text
$ spit-bash run pipeline.spitdag
1 job can start, 1 job may run if an upstream output changes
1 job will replace existing outputs:
  register: input changed
1 job will replace outputs that took 6m to make:
  register (6m)
Continue? [y/N] n
nothing was run
```

Without `--move-files`, `plan` and `run` say when moving would save work, as `1,018 jobs rerun only because their outputs are recorded at other paths; --move-files moves them instead`, counting the jobs a plan with the flag would place.

With `--move-files`, a job whose outputs the record says were made at other paths is planned `place` instead of `run`, and `run` moves those outputs to the paths the pipeline now gives. A job is placed only if its work and inputs are as recorded, each output is still as the run left it, no destination exists, and no job still uses the old path. The jobs that read a placed job's outputs see them as they will be once moved, so they are planned as if the files were already there, and `explain` shows them so. Otherwise the job runs, and `plan` says why its outputs are not moved, as `[2] not moved: final.txt is missing or changed since the run`. A placed job's moves are its action in the run: each output is renamed, so it keeps its size and time, and checked against the record, and the job is recorded at its new paths at once, so a run stopped afterwards finds them there. A placed job whose checks have not passed on its files runs them once the files are moved. Nothing is overwritten. If a move fails, the job's files are put back, the job fails, and the jobs that need it do not run. Moving back is what changing the rule back does. A move is not rerun, so a tool that writes its own paths into its output keeps the old path in it: use `--move-files` only for pipelines whose tools do not.

```text
$ spit-bash plan pipeline.spitdag --move-files --verbose
1 job will move its outputs, 1 job is current
[1] place   op1: moves work/upper.txt -> moved/upper.txt
[2] current op2: current
$ spit-bash run pipeline.spitdag --move-files
1 job will move its outputs, 1 job is current

Moving the outputs of 1 job
  [1] moved   op1: work/upper.txt -> moved/upper.txt
outputs were moved to the paths the pipeline now gives them; a tool that wrote those paths into its output is not rerun
0 done, 1 moved, 0 checked, 0 failed, 1 current, 0 blocked
```

When the pipeline loses a step or a subject, the outputs earlier runs made for its jobs stay on disk, and `plan` and `run` say so: they count the outputs that a record of a job the DAG no longer has names, that still exist, and that no job of the DAG makes or reads, with their total size, and `--verbose` lists them. A renamed product, dimension or source is not counted, since its job takes over the old record first. `run --remove-leftovers` deletes those that are as the run that made them left them, and forgets the records left with no output on disk; one changed since is kept and said to be. Nothing is deleted without the flag, and a run that starts nothing because a job is blocked deletes nothing either. A partial DAG reports none, as the jobs it leaves out would look lost.

```text
$ spit-bash plan pipeline.spitdag --verbose
1 job is current
[1] current op1: current
1 output (6 bytes) left over from jobs the pipeline no longer has; run --remove-leftovers to delete those unchanged since their run
  final.txt (6 bytes)
$ spit-bash run pipeline.spitdag --remove-leftovers
1 job is current
1 output (6 bytes) left over from jobs the pipeline no longer has; removing those unchanged since their run
0 done, 0 checked, 0 failed, 1 current, 0 blocked
```

Records name only what spit-bash wrote at the paths the pipeline gave. To find everything else, such as a file a failed tool wrote under a name of its own or outputs from before the state was kept, name a folder that holds only what the pipeline writes:

```text
$ spit-bash clean analysis.spitdag out
3 paths (9 bytes) under out that the DAG does not explain:
  out/old/ (3 bytes)
  out/upper/map_real.txt (4 bytes)
  sub=03, which no job of the DAG reads or makes:
    out/upper/sub=03.txt (2 bytes)
run again with --yes to delete them
```

A path is explained when it is a potential job's input or output, lies inside an output folder, is a review CSV or screen report, is the runner's folder or a file the pipeline is read from, or is an edited output kept as `NAME.edited-<time>` beside its job's output. This includes outputs held or excluded by current verdicts. Everything else under the folder is listed with its size, and a folder with nothing explained in it is listed once, with a `/`. Strays whose paths name an entity value no potential job reads or makes, such as a subject an `exclude` line left out, are listed under that value. A stray in a folder that is not writable says that it cannot be deleted. `clean` deletes nothing without `--yes`. With it, it deletes each stray, says which it could not delete and why, exiting with status 1, and forgets the records of jobs the DAG does not have whose outputs it deleted.

Naming the folder says that it holds outputs only, so `clean` refuses, deleting nothing, a folder that:

- is under neither the dataset root nor the output root;
- holds a source of the DAG, or lies inside one, as the dataset root itself does when it holds the sources;
- no job writes under;
- is cleaned with a partial DAG, or with `--entity`, `--product` or `--stage`, since the jobs left out would make their outputs look like strays.

Existing outputs with no successful record are run again; use `plan` to inspect this before `run`. For a dataset whose outputs were made before spit-bash was used, `spit-bash adopt` records every job whose inputs and outputs all exist as current, without running it. It trusts the files as they are, so adopt only outputs you know were made by the commands in the DAG. `explain` says of an adopted job `made by: no run recorded`.

Jobs with no command can only be skipped, or checked, when all their outputs already exist and are at least as new as their inputs; a missing or stale output blocks them. Failed verification, failed commands, failed checks and missing outputs fail the job and its dependents, and a failure once the command has started [removes the job's outputs](#when-a-job-fails). A partial DAG reports its `left_out` count while still planning and running its complete jobs.

A file is first compared by size and modification time, which costs one `stat`, and only a file whose stamp differs from the record is read and hashed. A folder's stamp is the number of files under it, at any depth, their total size and the latest modification time among them. The folders under it add nothing to the stamp, so touching one, adding an empty one, or renaming a file, which keeps its size and time, leaves the stamp as it was. A folder's hash covers the names of every file and folder under it and the bytes of its files, so where the stamp differs, any change of name or content under it counts. When the hash matches, `run` stores the new stamp, so the next plan need not hash the file again; `plan` itself changes nothing. If another tool changes a file while preserving both its size and its modification time, the runner will not notice. After a job runs, the hashes of its inputs and outputs are recorded; `adopt` records none, so a touched file of an adopted job counts as changed once. `--compare` on `plan` and `run` sets how a file counts as unchanged. `content`, the default, is as above. `stat` judges by size and time alone and records no hashes, so a job below a rerun always runs and a renamed product, dimension or source is not reused. `full` reads and hashes every recorded file of every job that would be skipped, trusting no size or time, and reruns a job whose record holds no hashes, such as one from `adopt`: it costs one full read of the data, for when a tool may have kept a file's size and time while changing it. The hash is xxh3_128 from the `xxhash` package, or BLAKE2b where that has no wheel, and each recorded hash names its algorithm, so one made with another is never taken as equal. The runner keeps its state and its [run folders](#run-history) in `.spit-bash` under the output root; `--dir DIR` names another folder for both. Keep one folder per output root.

The state file is a log of JSON lines: a header with its version, then one line each time a job is recorded or forgotten, and the last line for a job wins. Finishing a job appends a line rather than rewriting the file, and the end of a run compacts it to one line per job. `run`, `adopt`, `accept-tools`, `restore`, `prune` and `clean --yes` lock the state, so a second run on the same dataset stops with an error instead of overwriting the first one's records. Locking uses `flock`, so spit-bash runs on Linux and macOS, not Windows.

A SPIT DAG is validated with Pydantic before planning. Invalid field types, unknown fields, malformed argument parts and paths outside their root are rejected before any command runs.

## Development

From the repository's root:

```sh
cargo build
SPIT=$PWD/target/debug/spit uv run --no-project --with-editable './runner[test]' \
    python3 -m unittest discover -s runner/tests -v
uv run --no-project --with-editable './runner[test]' runner/tests/slurm-checks.sh
```

[uv](https://docs.astral.sh/uv/) runs each command in an environment with the runner installed from this checkout and its test dependencies, so nothing is installed into your Python.

Run both commands from the repository root to reproduce the CI type check.

```sh
uv run --no-project --with-editable './runner[types]' python3 -m mypy --config-file runner/pyproject.toml
```

The check targets Python 3.10 and requires exhaustive matches in the modules listed in `runner/pyproject.toml`. It also reads the types of imported runner modules, so annotations on their interfaces apply to the checked callers.

The test fixture under `tests/fixtures/` is a DAG from SPIT's `command_demo` example, written without a root. `tests/test_examples.py` runs the [examples](../examples/runnable/README.md) in `examples/runnable`, which SPIT's own tests keep in step with its syntax, with a real `spit`: set `SPIT` to the binary built from the same checkout, or put it on `PATH`, or the tests are skipped. CI builds `spit` from each commit and runs them, so a change to SPIT's DAG format and the runner's change to read it are tested together. The repository's CI also runs Messie over the runner's folders.
