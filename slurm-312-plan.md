# Plan: run spit-bash jobs through Slurm (#312)

The runner keeps planning, judging and the state; an **executor** starts jobs. The local executor is today's
behaviour. The Slurm executor submits ready jobs as arrays and reads each job's result file.

## Design

- **Executor boundary** (`execute/executor.py`). The scheduler asks an executor to `run` a job, or `check` it, and
  awaits a `Made` or `Failed`. The local executor holds today's `ReadyQueue` and `run_job`. The scheduler keeps the
  order of ready jobs, dependency failure, events and every write to the state.
- **One job on a node** (`spit-bash job BATCH INDEX`). The controller writes a batch file: the DAG trimmed to the
  batch's jobs, each task's job id, mode, attempt and token, and the run's options. A task runs its job with
  `execute/job.py`, hashes its inputs and outputs there, and writes a JSON result file atomically (temporary name,
  fsync, rename). The result names the job's key, fingerprint, attempt and token, so a stale result is never taken
  for a new one.
- **Namespace.** `ns` = the first 8 hex digits of a hash of the runner's folder. Slurm job names are
  `spit-<ns>-<operation>`; `--comment` holds `spit:<ns>:<token>`. Files go under `<runner folder>/slurm/`.
- **Ledger** (`<runner folder>/slurm/ledger.jsonl`). One line per submitted task and one when its result is
  recorded. A new controller harvests results left unread, adopts tasks still in the queue instead of submitting
  them again, and cancels those the run no longer needs.
- **Queue manners.** One `squeue --me` per interval for the whole run, filtered by the name prefix. `sacct` only for
  tasks that left the queue without a result, batched, and optional. A cap on tasks in the queue and a submission
  rate. Tasks of one operation and one set of resources gathered for a moment and submitted as one array.
  `--no-requeue`, and `--signal=B:TERM@<grace>` so a job puts back the earlier outputs before its time limit.
- **Time from history.** `--time` is the job's recorded seconds times a factor, within the profile's bounds, or the
  profile's default.
- **Retries.** A job killed for memory or time is resubmitted with more of it, as the profile says.
- **Small jobs.** The profile names operations the controller runs itself, with the local executor.
- **Profile** (TOML). Account, partition, default time and memory, extra options, setup lines, per-operation
  overrides, intervals, caps, retries, local operations.

## Steps

1. [ ] Executor boundary; the local executor behaves as now and every runner test passes unchanged.
2. [ ] Result files and `spit-bash job`, with tests for success, failure and a missing output.
3. [ ] Profile, ledger and namespace.
4. [ ] The Slurm executor: arrays, status, cap and rate, cancelling, `sacct` optional.
5. [ ] Harvest and adoption across controller restarts.
6. [ ] Retries with more memory or time; `--time` from history.
7. [ ] Local operations for small jobs.
8. [ ] Programs found after the profile's setup lines.
9. [ ] A fake Slurm, and tests against it in every runner test run.
10. [ ] Tests against the Docker cluster, and a CI job for them.
11. [ ] Documentation: `runner/README.md`, `README.md`.
