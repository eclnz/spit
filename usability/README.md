# Usability study

Can someone who has only the `spit` binary and its guide turn a pipeline problem into a correct plan, and how much does it cost them? To find out, we gave AI agents realistic tasks with nothing else: no source, no examples, no tests. We graded their plans against answer keys and logged every `spit` call they made.

The findings are in [FINDINGS.md](FINDINGS.md), written as a backlog of bugs, features and gaps in the guide. Designs that answer them are in [`design/`](design): so far, [removing inputs](design/removing-inputs.md), which replaces `skip`. This page describes how the study was run, what came out, and how to run it again.

## Scenarios

Each scenario has a brief written in the user's terms: what to produce and the exact command for each step, but never SPIT syntax. Each has a small dataset with deliberate irregularities, and an answer key: a reference pipeline and the `.spitdag` it resolves to.

| Scenario | Task | Irregularities | Tests | Jobs |
| --- | --- | --- | --- | --- |
| [s1-logs](harness/scenarios/s1-logs/brief.md) | Daily server logs → digests → weekly report per server → fleet report | A server misses a day; rotated, compressed and notes files | `many`/`vary`, path rules, scanning | 24 |
| [s2-cohort](harness/scenarios/s2-cohort/brief.md) | BIDS fMRI cohort: motion correction, brain extraction, coregistration, session and subject averages | A subject with one session to exclude; a session with an extra run; JSON sidecars | Recipes, `discover`, `skip` | 41 |
| [s2 follow-up](harness/scenarios/s2-cohort/followup.md) | Change request to the s2 agent: a new subject, a QC step, and one corrupted run to exclude | The raw file must stay in place | Cost of change | 60 |
| [s3-sweep](harness/scenarios/s3-sweep/brief.md) | Models × configs × per-config seeds → train, evaluate, summarise, leaderboards | Uneven seed sets; a single test set | `each` then `vary`, a source with no dimensions | 41 |
| [s4-sensors](harness/scenarios/s4-sensors/brief.md) | Calibrate readings, compare with a baseline, one report per station | Only calibration revision 3 may be used; baselines filed under different dates | `where`, `same`, `many` beside `one` | 19 |
| [s5-survey](harness/scenarios/s5-survey/brief.md) | Survey panel in ingest, model and publish phases | Waves 1, 2 and 10; a backup file | Stages, two outputs, `verify`, numeric order | 20 |
| [s6-diagnose](harness/scenarios/s6-diagnose/brief.md) | An inherited pipeline whose run fails: find every cause, then plan without the broken stores | One store with one week, one price list named `S07.json`, one missing | `artifacts`, error messages, `skip` | 22 |

Commands and output paths are fixed by each brief, so a correct plan is unique up to naming. The [grader](harness/grade.py) compares the multiset of each job's expanded command line, and its verify lines, with the key. Product names, job ids and path rules do not matter.

## Setup

- **What each agent had.** A sandbox holding the dataset, the brief, a guide (the README's user-facing sections followed by the [language reference](../docs/language-reference.md), built by [`build_guide.py`](harness/build_guide.py)), a report questionnaire, and `bin/spit`.
- **The wrapper.** `bin/spit` is a [wrapper](harness/shim.sh) that logs each call's arguments, output and exit code, with a snapshot of every `.spit`/`.spitin`/`.spitout` file at that moment.
- **Isolation.** Agents were told to stay inside their sandbox. Each transcript was audited afterwards for any access to the repository, the answer keys, another sandbox, or the web. None was found.
- **Models and runs.** Each scenario ran twice, each time in a fresh agent: once on a larger model (runs tagged `a`) and once on a smaller, faster one (runs tagged `b`), the latter standing in for a less capable reader. The s2 follow-up was sent to the same two agents after they finished, so they kept their earlier context, as a real user would.
- **Reports.** Agents answered the questionnaire in [`REPORT.md`](harness/REPORT.md). Where the agent tool would not let them write that file, they returned their answers as text, condensed into `notes.md` in each run's folder.

## Results

All 14 plans matched their keys exactly. No agent was confident and wrong: every agent rated its confidence 4 or 5, and every one was right.

| Scenario | Run a (larger model): jobs, `spit` calls | Run b (smaller model): jobs, `spit` calls |
| --- | --- | --- |
| s1-logs | 24/24, 9 | 24/24, 8 |
| s2-cohort | 41/41, 8 | 41/41, 6 |
| s2 follow-up | 60/60, +11 | 60/60, +18 |
| s3-sweep | 41/41, 10 | 41/41, 10 |
| s4-sensors | 19/19, 8 | 19/19, 9 |
| s5-survey | 20/20, 8 | 20/20, 5 |
| s6-diagnose | 22/22 and all 3 causes, 11 | 22/22 and all 3 causes, 20 |

Because the plans were all correct, the useful results are in the friction. That means the errors agents hit, the guesses they recorded, and the time the s2 follow-up took: excluding one run took each agent longer than the rest of the change request combined. See [FINDINGS.md](FINDINGS.md).

Each run's folder in [`results/`](results) holds:

- the pipeline and recipe files the agent wrote;
- its report (`REPORT.md`, or `notes.md` condensed from its reply);
- `ANSWER.md` for s6;
- `result.json`, with the grade, every `spit` call's diagnostics and the audit. For s2, the follow-up has its own `followup-notes.md` and `followup-result.json`.

### Limits

- **Every scenario was designed to be solvable,** and the briefs pin commands and output paths. A perfect pass rate therefore overstates ease of use. Scenario 3 was changed to two levels of rollup because one step cannot aggregate over two dimensions ([F9](FINDINGS.md#f9-aggregate-over-several-dimensions-in-one-step)).
- **Agents read the whole 470-line guide before starting.** A person skimming it would likely hit more of the [guide gaps](FINDINGS.md#guide-gaps).
- **One agent reported a garbled `note: Job 1` line.** The wrapper's logging caused it, not SPIT, and it is left out of the findings.

## Run it again

The second round also includes two variants: [s3-one-board](harness/scenarios/s3-one-board/brief.md) asks for one leaderboard across model and configuration (39 jobs), and [s4-vague](harness/scenarios/s4-vague/brief.md) gives the station task in less prescriptive terms (19 jobs). The s2 follow-up key now uses a recipe with `exclude` so the corrupted raw run stays in place. The other briefs and datasets stay the same, making their results comparable with round 1.

```sh
cargo build --release
usability/harness/rebuild_keys.sh              # every answer key still resolves to the same jobs
usability/harness/make_run.sh s1-logs a        # prints the sandbox path
```

`make_run.sh` builds a sandbox under `SPIT_TRIALS` (default `/tmp/spit-trials`). It keeps the real binary and the call logs under `SPIT_PRIVATE` (default `/tmp/spit-trials-private`), out of the agent's reach. Give an agent this prompt, with the sandbox path filled in:

> Your working folder is `<sandbox>`. Start by reading `<sandbox>/TASK.md` and follow it exactly, including its rules about staying inside that folder. Use absolute paths in every command, since your shell's working directory may reset between commands.

For the s2 follow-up, copy `harness/scenarios/s2-cohort/addition/sub-06` into the sandbox's `data/` once the agent has finished, save its `plan.spitdag` elsewhere for grading, and send it [`followup.md`](harness/scenarios/s2-cohort/followup.md).

Then grade every run, summarise its calls, and audit its transcript:

```sh
usability/harness/analyze.py runs.json
```

The format of `runs.json` is described at the top of [`analyze.py`](harness/analyze.py). Results go to `SPIT_PRIVATE/results`.

After a change to the resolver or the language, run `rebuild_keys.sh` first. A key that no longer resolves to the same jobs means its scenario needs updating before the next round. `rebuild_keys.sh --write` replaces the stored keys.
