# Troubleshooting

This page lists what you see when something goes wrong, what it usually means, and what to do. The first half is about planning, where SPIT stops with an error before anything runs, and the second half is about runs. Most of the messages here are quoted from the [weather network](walkthrough.md) and the examples in the repository.

A mistake in the text of a pipeline or recipe, such as a clause in the wrong place, a misspelt placeholder or an extension that disagrees with its operation, stops `spit check` at its line with an error that says what to write instead, and the [editor](editor.md) shows the same errors as you type. This guide teaches the rules you need to write a pipeline, and leaves those cases to the errors, so read the error first.

Two commands answer most questions. `spit artifacts dataset.spitin` explains every artifact that cannot be made, and `spit-bash explain ID dataset.spitin` explains why one job will rerun. Start there, and use the entries below to read what they say.

## Planning

### No artifact for an input

```text
error: weather.spit: line 43, column 31: no `location` artifact for input `location` of `reduce` at [region=coast,site=dune]
```

A job has no file to read for one of its inputs, here the location of the site `coast/dune`. Look for the cause in this order:

1. The file is not where the source rule says. `spit inputs dataset.spitin --unmatched` lists the files that no rule reads. A name that differs in case or spelling, such as `Site.json` for `site.json`, shows up there.
2. The file is there, but its dimension values differ from the job's. `spit artifacts` points at a near miss, such as ``ref[group=beta] exists; its `group` differs only in letter case``. Values match as written, so `3` and `03` are different.
3. A recipe line removed it. Check the `note: excluded ...` lines, and the reviews.

[Plans](plans.md#missing-work) shows the whole chain, and `spit dag --partial` plans the jobs that can be made while you fix it.

### More than one artifact matches

```text
error: line 9, column 28: more than one `baseline` artifact matches input `baseline` of `compare` at [region=coast,site=dune]; select one with `@where(...)`
```

An input matched several artifacts, because it has a dimension the driving input lacks. Add a selector that leaves one, such as `baseline @where(revision=2)`, or `@same(region)` to match on one dimension only. See [matching](pipelines.md#matching).

### Coverage requirement failed

```text
error: source coverage for `location` at [region=coast,site=dune]: expected exactly 1 artifact(s), found 0
```

A `require` line in the recipe is not met, here by a site with no location file. The note above it, when there is one, usually names the file SPIT could not read. Fix the data, or change the rule if it is wrong. A `require` that finds no group at all is also an error, since a check of nothing is not a pass.

### A collection is too small

```text
error: line 9, column 17: input `days` of `clean` needs at least 2 artifacts at [region=coast,site=dune], found 1
```

A `@min(2)` on the port rejects the group. Either the group really has too few files, or most of them were not found: check `spit inputs --unmatched`. `spit dag --partial` leaves the group out and plans the rest.

### Fewer jobs than expected

`spit dag --counts` shows a count for every step, and a low or zero count points to the step whose driving source has no files. Work backwards:

- `spit inputs` says how many artifacts it found. If it is lower than the files on disk, a source rule matches too little.
- `exclude` lines are reported as `note: excluded ...`, and a review withholds artifacts with `note: review ... holds back ...` or `... rejected ...`.
- A `require` or `@min` that fails is an error, not a low count.

### The order of dimensions is not set

<!-- spit-doc: output
spit check order.spit || true
--- order.spit
source cleaned : Readings [region, site]
source threshold : Threshold [alert]
default entities: {@key}={@value} separated "_"
default path: out/{@product}/{@entities}
operation tally(series: Readings, threshold: Threshold) -> Count
alert_days = tally(cleaned, threshold @each(alert))
-->

```text
error: line 6, column 1: `alert_days` has dimensions `region` and `alert`, which no source orders; declare the order once with `dimensions [region, site, alert]`
```

No source holds two dimensions of one product together, so SPIT cannot tell which comes first. Write the line it suggests. See [dimension order](artifacts.md#dimension-order).

### A path rule is wrong

A path rule is checked when the pipeline loads, so these errors come before any data:

<!-- spit-doc: output
spit check entities.spit || true
--- entities.spit
source cleaned : Readings [region, site]
default path: out/{@product}/{@entities}
operation smooth(series: Readings) -> Readings
smoothed = smooth(cleaned)
-->

```text
error: line 2, column 30: path rule for `smoothed` uses `{@entities}`, but no `default entities:` spells it; add one, as `default entities: {@key}={@value} separated "__"`
```

- **`{@entities}` has no spelling.** Add a `default entities:` line, as the message says.
- **Two products bind to the same path.** Include `{@product}`, or every dimension that tells them apart. See [escaping and collisions](artifacts.md#escaping-and-collisions).
- **`{@stage}` outside a stage.** A product made outside every stage has no stage, so write `[{@stage}/]` in the rule to leave the directory out.
- **A dimension the product lacks.** A rule may use only the dimensions its product has, unless the placeholder is inside an optional `[...]` group.
- **An extension that does not match.** A path that ends with an extension other than the one the operation declares is an error, since the tool writes a different file.

### The types do not fit

```text
error: line 50, column 32: type mismatch at `compare.baseline`: product `regional` is Readings<Clean>, expected Readings<SeaLevel>
```

A step passes a product of one type where a port wants another. Check the argument order, and the types in the signature. Types are optional, so remove one only if it is wrong. See [types](artifacts.md#types).

### A pipeline cycle

```text
error: line 6, column 1: pipeline cycle: smoothed -> adjusted -> smoothed
```

Steps read each other in a loop. Break one dependency, or add a separate input.

### An unknown operation

<!-- spit-doc: output
spit check unknown.spit || true
--- unknown.spit
source cleaned : Readings [site]
operation smoothe(series: Readings) -> Readings
command smooth: smooth_tool {series} {@output}
-->

```text
error: line 3, column 17: command refers to unknown operation `smooth`
```

The name after `command` matches no declared operation, usually a misspelling. A check named in `@check(...)` must be declared too, and must use `{@path}` and every parameter it declares. A command must use every output. An operation must be declared before the first step that calls it.

### A line is in the wrong file

<!-- spit-doc: output
spit check step.spitin || true
--- survey.spit
source image : Image [sub]
path image: data/{sub}.nii.gz
--- step.spitin
pipeline survey.spit
root data
smoothed = smooth(image)
-->

```text
error: line 3, column 1: a step belongs in the .spit pipeline, which every dataset shares; a .spitin binds it to one dataset with its `root`, source paths, and `inputs:`, `exclude` and `require` rules
```

A pipeline holds what to make, and a recipe holds what is in this dataset. Steps, operations, checks and types belong in the pipeline. `exclude`, `require`, `review ... from` and `tools from` belong in the recipe. A source's path may be in either file, but not both: ``image` has path rules in both .spit and .spitin`.

### The dataset root is not a folder

```text
warning: line 2, column 6: dataset root `data` is not a folder here; `spit inputs` and `spit dag` stop with an error unless it is one where they run
```

A `root` in a recipe is relative to the recipe, and in a pipeline to the pipeline. `--root` is relative to the folder you run in. Run the command where the folder exists, or fix the path.

## Runs

### Nothing was run

```text
39 jobs are blocked, 59 jobs are current
blocked: reduce x16: program not found: python4 (23 more depend on them)
nothing was run: 39 jobs are blocked. Fix the cause, or pass --partial to run the jobs that are not.
```

A job needs a program that is not on `PATH`, or an input that does not exist yet, and so are the jobs below it. The line that starts `blocked:` names the cause once, however many jobs it holds up. Install the program, or correct its name. `run --partial` runs the jobs that are not blocked.

### A job failed

```text
[3] failed  clean region=coast site=dune: check day_of_hours on input days coast/dune/2025-06-26.csv failed: exited with status 1
[3] held up 21 other jobs
```

The failure names the check or command that failed and the last lines of its log. The jobs that depend on it did not run, and are counted as `not run`, not as failures of their own. Read the whole log with `spit-bash log dataset.spitin --product cleaned`, fix the file or the command, and run again: a job that failed keeps its earlier record, so only what changed reruns. `run --keep-failed` leaves a failed job's files for you to look at.

### A tool used too much memory

<!-- spit-doc: plain -->

```text
command used 212M of memory, more than the 64M its job declares, and was killed
```

The job's `resources` line declares less memory than the tool needs. Raise it, on the largest input you expect. `--no-enforce-memory` turns the limit off.

### Everything reruns

`spit-bash explain ID dataset.spitin` for any job says what changed: the command, an input, a tool or an output. Common causes are:

- The **command** changed, for example a path rule that changed every job's paths. `--move-files` moves outputs to new paths instead of rerunning.
- A **tool's identity** changed. A probe that prints something different on every call, such as a date, reruns everything that depends on it. The runner shows the lines that changed before anything else.
- An **input** really changed. Content is compared when the size or time differ, so a `touch` is harmless.

### Nothing reruns after an edit

A job reruns only when a file it declares changes. A script named in a command by a relative path or `{@here}` is not declared, so editing it reruns nothing, and `spit dag` notes each such file:

<!-- spit-doc: plain -->

```text
note: operation `reduce` runs `{@here}/steps/reduce.py`, which it doesn't declare, so editing the file reruns nothing; to rerun on a change, declare it a `here` source and pass it to `reduce` as a source
```

Declare the script as a source and pass it to the operation, as the [terrain and classifier analyses](operations.md#command-templates) do. The same holds for a settings file a command reads.

### The runner rejects a plan

The runner reads exactly one version of the plan format. A `.spitdag` made by a different `spit` is refused before anything runs. Use a `spit` and a `spit-bash` from the same release, and make the plan again.

### Where to look next

- [Plans](plans.md) for inspecting what SPIT will do.
- [Runs](runs.md) for what the runner decides and keeps.
- [Artifacts](artifacts.md), [Operations](operations.md), [Pipelines](pipelines.md) and [Datasets](datasets.md) for the rule behind an error.
