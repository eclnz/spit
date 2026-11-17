# Runs

SPIT writes a plan and never runs it. `spit-bash` is the runner that carries a plan out, on one machine or as Slurm jobs on a cluster. It decides which jobs need to run, runs them in the right order and as many at once as the machine can hold, and keeps a record so that the next run does only the work that changed. This page covers what you can ask the runner, how it decides, and how to manage what it leaves behind. The examples come from the [weather network](walkthrough.md). The [runner reference](https://github.com/eclnz/spit/blob/dev/runner/README.md) lists every option.

`spit-bash` takes what `spit dag` takes: a recipe, a pipeline with a root, or a saved `.spitdag`, with `-` reading a plan from standard input. Given a recipe or pipeline, it asks `spit dag` for the plan first, so it needs `spit` on `PATH`, and a saved plan needs nothing else. It runs on Linux and macOS, since it locks its record with `flock`, and it needs Python 3.10 or newer.

## Run inspection

`plan` says what a run would do and changes nothing, and `explain` says why for one job. After the frost threshold in the weather network is edited:

<!-- spit-doc: fragment -->

```sh
spit-bash plan weather.spitin
spit-bash explain 57 weather.spitin
```

```text
since the last run: 1 threshold changed (alert=frost)
16 jobs can start, 3 jobs may run if an upstream output changes, 79 jobs are current
16 jobs will replace existing outputs:
  tally x16: input changed
```

`plan` first says what changed since the last run, in the terms of the data. It then counts the jobs, and `--verbose` lists each one with a status and the reason:

- `ready` means the job can start now.
- `waits` means a job upstream will rewrite something this one reads.
- `may run` means the job is current as it stands, but a job it reads from will run, so whether it must run is known only once that job has finished.
- `current` means it can be skipped.
- `check` means only its checks changed, so the runner tests the existing files without running the command again.
- `place` means its outputs will be moved (see [Reuse](#reuse)).
- `blocked` names a prerequisite that prevents the work, such as a program that is not installed.

`plan` exits with status 1 when any job is blocked. `plan --json` gives the same information for a script: each job's status, its reason in words, a code for the reason that stays the same when the words change, and the jobs it waits for.

Use a job's number from the plan with `explain` to see why it was decided as it was. It runs nothing and writes nothing:

<!-- spit-doc: plain -->

```text
Job 57: tally alert=frost region=coast site=alder
  decision: run, input changed
  made by run 2026-11-15T07-14-41; log at data/results/.spit-bash/runs/2026-11-15T07-14-41/jobs/alert_days/alert=frost_region=coast_site=alder.log
  fingerprint: same (recorded 44e79dd682d9dd89, now 44e79dd682d9dd89)
  checks: same (recorded 19ffef35a7e8baca, now 19ffef35a7e8baca)
  inputs that differ from the record:
    alerts/frost.json
      recorded: 15 bytes, mtime_ns 1794726880690037148
      now:      16 bytes, mtime_ns 1794726938558040588
      content differs
  outputs that differ from the record:
    none differs from the record
```

The job's command and checks are unchanged. One input differs, and its content is not what the record holds.

## Rerun decisions

A job runs when any of these changed since its last successful run:

- its **command**, or the shape of what it reads and makes;
- an **input** file, or the file an upstream job rewrote;
- a **tool** it depends on, as recorded by a `.spitenv`;
- an **output**, which is missing or was edited by hand.

Otherwise it is skipped. By default the runner compares a file's size and modification time first, and reads the content only where those differ, so a `touch` or a `git checkout` that leaves the bytes alone reruns nothing. `--compare` sets the rule:

- `content`, the default, is as above.
- `stat` judges by size and time alone. It reads no files, but cannot tell that a rewritten file holds the same bytes.
- `full` reads and hashes every recorded file, trusting no size or time. It costs one full read of the data, for when a tool may have kept a file's size and time while changing it.

**Early stop.** The runner stops a chain of reruns when a job writes the same bytes as before. Reformat the frost threshold file without changing its value, `{"below":  0.5}` for `{"below": 0.5}`, and the 16 tally jobs rerun because their input changed. They write the same counts as before, so the three jobs below them are skipped:

<!-- spit-doc: plain -->

```text
  [91] skip    tabulate_alerts: upstream output unchanged
  [96] skip    fig::alert_days: upstream output unchanged
  [98] skip    fig::compose: upstream output unchanged
16 done, 0 checked, 0 failed, 79 current, 3 kept (upstream reran with unchanged output), 0 blocked
```

`--compare stat` turns this off, since it has no content to compare. A change to artifact checks alone gives a `check` job, which tests the existing files and runs no command.

## Selection

`--entity`, `--product` and `--stage` choose part of a plan, so a pipeline can be tried on one site or one stage before everything runs:

<!-- spit-doc: fragment -->

```sh
spit-bash plan weather.spitin --entity region=coast
spit-bash plan weather.spitin --stage alerts --product alert_days
```

The first chooses 27 of the 98 jobs, the coast's, and the second chooses the 32 tallies and the 16 `clean` jobs they need:

```text
chose 27 of 98 jobs
chose 32 of 98 jobs, and 16 they need
```

| Option | Chooses jobs with |
| --- | --- |
| `--entity DIM=VALUE[,DIM=VALUE]` | An output that has every one of these values |
| `--product NAME` | An output of this product |
| `--stage NAME[/NAME]` | This stage, or a stage inside it |

Repeat an option to match any of its values. Different options must all hold for the same output. A selection includes the upstream jobs the chosen ones need, and nothing downstream, so `--entity region=coast` leaves out the league and the figure, which read every region. A name that matches nothing is an error that lists the names that would, so a misspelt dimension does not just choose nothing.

Choosing jobs does not change how a job is judged: a chosen job is skipped when it is current. `--force` reruns the chosen command jobs even when current, or every command job when nothing is chosen, and their prerequisites still follow the normal rules. `--stage compare --force` reruns the 40 jobs of that stage and leaves the 16 `clean` jobs it reads alone. A job outside the selection that reads an output a chosen job rewrote may be out of date afterwards, and the summary says how many.

## Scheduling

Jobs that are ready run together while their cores and memory fit. Set the run's budget with `-j` or `--cores`, and `--memory`:

<!-- spit-doc: fragment -->

```sh
spit-bash run weather.spitin -j 8 --memory 8G
```

The weather network's 98 jobs run in about six seconds on eight cores.

- `-j` defaults to the number of CPUs, and `--memory` to the machine's memory.
- A job's own request comes from its operation's [`resources`](operations.md#resources) line, and a job that declares none takes one core and no memory.
- A job starts only when its cores and memory are free beside those of the jobs already running, and holds them until its checks finish.
- A job that asks for more memory than the whole run has is refused before anything starts, and `run` exits with status 2. A job that asks for more cores than `-j` runs alone, once no other job is running.
- When more jobs are ready than fit, the one at the head of the longest chain of work still to come starts first, so a long chain does not finish alone at the end of a run.
- `--no-enforce-memory` turns off the check that kills a job holding more memory than it declared, and the refusal above.

## Execution

`run` plans first and acts on the plan:

- Commands run from the output root, which the runner makes. Output folders and parent directories are made when a job starts. An output folder declared `from` an input starts as a copy of that input instead, made again for each attempt.
- A job's input checks and `verify` commands run first, in order, then its commands, then its output checks once every declared output exists.
- A job that exits successfully but leaves a declared output missing fails. The failure names the output, and any file that appeared in its place.
- A job starts only when its producers have finished successfully.

Each job's output goes to a log, so parallel jobs do not mix on the terminal. The terminal shows each start, finish and failure, and the last lines of a failed job's log. If one weather file is truncated, the check on the source stops the job before `clean` runs, and the jobs below it do not start:

<!-- spit-doc: plain -->

```text
[3] failed  clean region=coast site=dune: check day_of_hours on input days coast/dune/2025-06-26.csv failed: exited with status 1
[3] held up 21 other jobs
0 done, 0 checked, 1 failed, 21 not run (an upstream job failed), 76 current, 0 blocked
```

**Blocked jobs.** A job is blocked when an external input is missing or a program it needs is not found, and so are the jobs below it. While any job is blocked, `run` starts nothing, so no tool runs on a missing input. With a misspelt program in the `reduce` command:

<!-- spit-doc: plain -->

```text
39 jobs are blocked, 59 jobs are current
blocked: reduce x16: program not found: python4 (23 more depend on them)
nothing was run: 39 jobs are blocked. Fix the cause, or pass --partial to run the jobs that are not.
```

`run --partial` runs every job that is not blocked, which suits a dataset that is still being collected. It still exits with status 1 while jobs are blocked.

**Failures.** After a job's first command starts, any failure removes the job's declared outputs, so a truncated file is never taken for a finished one, and puts back the earlier version it moved aside. The job's log records both. A job that fails before its first command, at an input check or `verify`, leaves its outputs as they were. `--keep-failed` leaves a failed job's files in place for debugging, and the next run runs it again. Ctrl-C stops every running job and cleans up in the same way. Restore the broken file and nothing needs to run, because the job that failed kept its earlier record.

**Replacing slow outputs.** At a terminal, `run` asks before it replaces the outputs of a job that took at least two minutes last time. `--ask-over` changes the time and `--yes` answers for you.

**Exit status.**

| Status | Meaning |
| --- | --- |
| 0 | Every job that ran succeeded |
| 1 | A job failed or did not run, a job is blocked, or you declined to replace slow outputs |
| 2 | `spit dag` failed, or a job needs more memory than the run has |
| 130, 143 | The run was interrupted or terminated |

## Reuse

The runner records each successful job in `.spit-bash/` under the output root, by the products and values it made, not by their paths. Three commands reuse work that the record cannot match on its own:

- **Tool identities.** `spit-bash accept-tools weather.spitin` records the tools as they are for every job whose only difference is the tool, so an upgrade that should not rerun finished work reruns nothing.
- **Existing outputs.** `spit-bash adopt weather.spitin` marks a job current when all its inputs and outputs exist, without running it. It trusts the files, so adopt only outputs you know the DAG's commands made. A later `--compare full` still reruns an adopted job once, since it recorded no content.
- **Moved outputs.** When path rules change, a job's outputs are missing from the new paths. `plan --move-files --verbose` shows the jobs that would move their files as `place`, and `run --move-files` moves them, only where the work and inputs are as recorded and nothing is overwritten:

<!-- spit-doc: plain -->

```text
98 jobs will move their outputs
[ 1] place   clean region=coast site=alder: moves coast/alder/cleaned.csv -> sites/coast/alder/cleaned.csv
```

Here the weather pipeline's `default path:` gained a `sites/` prefix, so every job's outputs move.

Use `--move-files` only for outputs that remain valid at a new path. A tool that wrote its own path into a file is [`anchored`](operations.md#tools) instead, and reruns when the path changes.

## Maintenance

Each run gets a folder under `.spit-bash/runs/`, named by when it started. It holds the run's log, the plan it acted on, a summary of how it ended, each job's log, and the outputs the run replaced. Jobs that replace outputs keep the earlier version there, and say where, and how much disk it uses. `run --no-keep-replaced` deletes the earlier version instead, for outputs too large to keep twice.

- **Logs.** `spit-bash log weather.spitin --product league` lists each chosen job's logs across runs, newest first, and marks the one that made its current outputs. `explain` shows the log behind a current job. `--timestamp-logs` starts every line of a log with the time it arrived.
- **Restore.** `spit-bash restore weather.spitin --run RUN` puts back the outputs that run replaced, with their records. What was there is kept in a new run, so a restore can be undone the same way. A restored file differs from what its readers' records hold, so the next plan reruns what reads it.
- **Prune.** `spit-bash prune weather.spitin --keep-runs N` removes all but the newest `N` runs, and keeps the log behind each current output. The replaced copies a pruned run kept are deleted with it. Nothing is pruned unless asked.
- **Clean.** `spit-bash clean weather.spitin data/results` lists every file and folder under the folder that the plan does not explain, such as a file a failed tool wrote under a name of its own. Name a folder that holds only the pipeline's outputs. It deletes nothing until you add `--yes`:

<!-- spit-doc: plain -->

```text
1 path (0 bytes) under . that the DAG does not explain:
  stray.txt (0 bytes)
run again with --yes to delete them
```

A site that an `exclude` line or a review left out has its files listed under its value, so a site meant to come back is not deleted unseen. `clean` refuses a folder that holds a source, or a partial plan, since jobs left out would make their outputs look like strays.

`run --remove-leftovers` deletes the outputs of jobs that are not in the pipeline, when they are as the run that made them left them. `--dir DIR` keeps the record and the run folders somewhere other than `.spit-bash`; keep one folder for each output root. A second run on the same dataset stops with an error while the first holds its record, so run one at a time.

`spit-bash run --executor slurm --profile cluster.toml` runs each job as a Slurm job instead, keeping the same record, so a dataset can move between a laptop and a cluster: the [runner reference](https://github.com/eclnz/spit/blob/dev/runner/README.md#running-on-a-slurm-cluster) covers the profile and how the runner uses the queue. Another runner needs only the [DAG format](../spitdag.md): the plan is the whole contract between SPIT and a runner.
