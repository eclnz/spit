# Runs

SPIT writes a plan and never runs it. `spit-bash` is the runner that carries a plan out on one machine. It decides which jobs need to run, runs them in the right order and as many at once as the machine can hold, and keeps a record so that the next run does only the work that changed. This page covers what you can ask the runner, how it decides, and how to manage what it leaves behind. The [runner reference](https://github.com/eclnz/spit/blob/dev/runner/README.md) lists every option.

`spit-bash` takes what `spit dag` takes: a recipe, a pipeline with a root, or a saved `.spitdag`, with `-` reading a plan from standard input. Given a recipe or pipeline, it asks `spit dag` for the plan first, so it needs `spit` on `PATH`, and a saved plan needs nothing else.

## Run inspection

`plan` says what a run would do and changes nothing, and `explain` says why for one job:

<!-- spit-doc: fragment -->

```sh
spit-bash plan examples/runnable/lines/lines.spitin --verbose
spit-bash explain 2 examples/runnable/lines/lines.spitin
```

`plan` first says what changed since the last run, in the terms of the data, such as `1 shard changed (group=alpha part=01)`. It then counts the jobs, and `--verbose` lists each one with a status and the reason:

```text
since the last run: 1 shard changed (group=alpha part=01)
1 job can start, 1 job may run if an upstream output changes, 3 jobs are current
[1] ready   sort_lines group=alpha part=01: input changed
[2] current sort_lines group=alpha part=02: current
[3] current sort_lines group=beta part=01: current
[4] may run merge group=alpha: runs if an upstream output changes
[5] current merge group=beta: current
```

- `ready` means the job can start now.
- `waits` means a job upstream will rewrite something this one reads.
- `may run` means the job is current as it stands, but a job it reads from will run, so whether it must run is known only once that job has finished.
- `current` means it can be skipped.
- `check` means only its checks changed, so the runner tests the existing files without running the command again.
- `place` means its outputs will be moved (see [Reuse](#reuse)).
- `blocked` names a prerequisite that prevents the work, such as a program that is not installed.

`plan --verbose` exits with status 1 when any job is blocked. `plan --json` gives the same information for a script, with a stable code for each reason.

Use a job's number from the plan with `explain` to see why it was decided as it was: the recorded and current fingerprint, the checks, and each input or output file that differs from the record, with sizes and times. It runs nothing and writes nothing.

## Rerun decisions

A job runs when any of these changed since its last successful run:

- its **command**, or the shape of what it reads and makes;
- an **input** file, or the file an upstream job rewrote;
- a **tool** it depends on, as recorded by a `.spitenv`;
- an **output**, which is missing or was edited by hand.

Otherwise it is skipped. `explain` compares the recorded job with the current one and names what changed. By default the runner compares a file's size and modification time first, and reads the content only where those differ, so a `touch` or a `git checkout` that leaves the bytes alone reruns nothing. `--compare` sets the rule:

- `content`, the default, is as above.
- `stat` judges by size and time alone. It reads no files, but cannot tell that a rewritten file holds the same bytes.
- `full` reads and hashes every recorded file, trusting no size or time. It costs one full read of the data, for when a tool may have kept a file's size and time while changing it.

**Early stop.** The runner stops a chain of reruns when a job writes the same bytes as before. Change one alpha shard in the [first run](first-run.md), and its sort job reruns. If that sorted file comes out identical, because the change only repeated a line, the merge that reads it is skipped:

```text
  [2] start   sort_lines group=alpha part=02
  [2] done    sort_lines group=alpha part=02 in under 1s; 1 left, under 1s
  [4] skip    merge group=alpha: upstream output unchanged
```

`--compare stat` turns this off, since it has no content to compare. A change to artifact checks alone gives a `check` job, which tests the existing files and runs no command.

## Selection

`--entity`, `--product` and `--stage` choose part of a plan, so a pipeline can be tried on one subject before every subject runs:

<!-- spit-doc: fragment -->

```sh
spit-bash plan examples/runnable/lines/lines.spitin --entity group=beta --product merged
```

This selects beta's merge job and the sort job it needs.

| Option | Chooses jobs with |
| --- | --- |
| `--entity DIM=VALUE[,DIM=VALUE]` | An output that has every one of these values |
| `--product NAME` | An output of this product |
| `--stage NAME[/NAME]` | This stage, or a stage inside it |

Repeat an option to match any of its values. Different options must all hold for the same output. A selection includes the upstream jobs the chosen ones need, and nothing downstream. A name that matches nothing is an error that lists the names that would, so a misspelt dimension does not just choose nothing.

Choosing jobs does not change how a job is judged: a chosen job is skipped when it is current. `--force` reruns the chosen command jobs even when current, or every command job when nothing is chosen, and their prerequisites still follow the normal rules. A job outside the selection that reads an output a chosen job rewrote may be out of date afterwards, and the summary says how many.

## Scheduling

Jobs that are ready run together while their cores and memory fit. Set the run's budget with `-j` or `--cores`, and `--memory`:

<!-- spit-doc: fragment -->

```sh
spit-bash run dataset.spitin -j 4 --memory 8G
```

- `-j` defaults to the number of CPUs, and `--memory` to the machine's memory.
- A job's own request comes from its operation's [`resources`](operations.md#resources) line, and a job that declares none takes one core and no memory.
- A job starts only when its cores and memory are free beside those of the jobs already running, and holds them until its checks finish.
- A job that asks for more memory than the whole run has is refused before anything starts. A job that asks for more cores than `-j` runs alone, once no other job is running.
- When more jobs are ready than fit, the one at the head of the longest chain of work still to come starts first, so a long chain does not finish alone at the end of a run.

## Execution

`run` plans first and acts on the plan:

- Commands run from the output root, which the runner makes. Output folders and parent directories are made when a job starts.
- A job's input checks and `verify` commands run first, in order, then its commands, then its output checks once every declared output exists.
- A job that exits successfully but leaves a declared output missing fails. The failure names the output, and any file that appeared in its place.
- A job starts only when its producers have finished successfully.

Each job's output goes to a log, so parallel jobs do not mix on the terminal. The terminal shows each start, finish and failure, and the last lines of a failed job's log:

```text
5 done, 0 checked, 0 failed, 3 current, 0 blocked
```

**Blocked jobs.** A job is blocked when an external input is missing or a program it needs is not found, and so are the jobs below it. While any job is blocked, `run` starts nothing, so no tool runs on a missing input:

<!-- spit-doc: fragment -->

```text
5 jobs are blocked
blocked: sort_lines x3: program not found: sortx (2 more depend on them)
nothing was run: 5 jobs are blocked. Fix the cause, or pass --partial to run the jobs that are not.
```

`run --partial` runs every job that is not blocked, which suits a dataset that is still being collected.

**Failures.** After a job's first command starts, any failure removes the job's declared outputs, so a truncated file is never taken for a finished one, and puts back the earlier version it moved aside. The job's log records both. `--keep-failed` leaves a failed job's files in place for debugging, and the next run runs it again. Ctrl-C stops every running job and cleans up in the same way.

**Replacing slow outputs.** At a terminal, `run` asks before it replaces the outputs of a job that took at least two minutes last time. `--ask-over` changes the time and `--yes` answers for you.

## Reuse

The runner records each successful job in `.spit-bash/` under the output root, by the products and values it made, not by their paths. Three commands reuse work that the record cannot match on its own:

- **Tool identities.** `spit-bash accept-tools dataset.spitin` records the tools as they are for every job whose only difference is the tool, so an upgrade that should not rerun finished work reruns nothing.
- **Existing outputs.** `spit-bash adopt dataset.spitin` marks a job current when all its inputs and outputs exist, without running it. It trusts the files, so adopt only outputs you know the DAG's commands made. A later `--compare full` still reruns an adopted job once, since it recorded no content.
- **Moved outputs.** When path rules change, a job's outputs are missing from the new paths. `plan --move-files --verbose` shows the jobs that would move their files as `place`, and `run --move-files` moves them, only where the work and inputs are as recorded and nothing is overwritten:

```text
[1] place   sort_lines group=alpha part=01: moves output/sorted/group=alpha_part=01.txt -> moved/sorted/group=alpha_part=01.txt
```

Use `--move-files` only for outputs that remain valid at a new path. A tool that wrote its own path into a file is [`anchored`](operations.md#tools) instead, and reruns when the path changes.

## Maintenance

Each run gets a folder under `.spit-bash/runs/`, named by when it started. It holds the run's log, the plan it acted on, a summary of how it ended, each job's log, and the outputs the run replaced. Jobs that replace outputs keep the earlier version there, and say where, and how much disk it uses. `run --no-keep-replaced` deletes the earlier version instead, for outputs too large to keep twice.

- **Logs.** `spit-bash log dataset.spitin --product merged` lists each chosen job's logs across runs, newest first, and marks the one that made its current outputs. `explain` shows the log behind a current job.
- **Restore.** `spit-bash restore dataset.spitin --run RUN` puts back the outputs that run replaced, with their records. What was there is kept in a new run, so a restore can be undone the same way. A restored file differs from what its readers' records hold, so the next plan reruns what reads it.
- **Prune.** `spit-bash prune dataset.spitin --keep-runs N` removes all but the newest `N` runs, and keeps the log behind each current output. The replaced copies a pruned run kept are deleted with it. Nothing is pruned unless asked.
- **Clean.** `spit-bash clean dataset.spitin FOLDER` lists every file and folder under `FOLDER` that the plan does not explain, such as a file a failed tool wrote under a name of its own. Name a folder that holds only the pipeline's outputs. It deletes nothing until you add `--yes`:

<!-- spit-doc: fragment -->

```text
1 path (0 bytes) under output that the DAG does not explain:
  output/stray.txt (0 bytes)
run again with --yes to delete them
```

A subject an `exclude` line or a review left out has its files listed under its value, so a subject meant to come back is not deleted unseen. `clean` refuses a folder that holds a source, or a partial plan, since jobs left out would make their outputs look like strays.

`run --remove-leftovers` deletes the outputs of jobs that are not in the pipeline, when they are as the run that made them left them. A second run on the same dataset stops with an error while the first holds its record, so keep one run at a time for each output root.

Another runner, such as one for a cluster, needs only the [DAG format](../spitdag.md): the plan is the whole contract between SPIT and a runner.
