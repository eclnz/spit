# Usability study: second round

The second round used the updated SPIT binary and guide after the usability work on the `usability` branch. Each of eight scenarios had two fresh, isolated agent participants. The two cohort participants then received the change request in their existing sessions. The [run archives](results.zip) contain each plan, pipeline and recipe, report, and grading result.

## Method and limits

Each participant received only a trial folder made by `harness/make_run.sh`: the scenario data, `TASK.md`, `GUIDE.md`, `REPORT.md`, and a logging `bin/spit` wrapper. They were instructed to stay inside that folder and not use the repository or web. Participants had no prior conversation context; the cohort follow-up deliberately kept its participant's context. All participants used the same model, so the `a` and `b` runs are replicates, unlike round 1's larger and smaller models. The comparison in this report is directional, not a controlled model-to-model comparison.

The grader compares expanded job and verification commands with the reference key. It does not evaluate whether the planned commands would process real data correctly. We retained the plans and wrapper telemetry but did not have participant tool transcripts, so the repository, other-sandbox, and web access audit from round 1 could not be repeated. The `audit.error` field in each result records this limit; a printed zero violations without a transcript does not mean a completed audit.

The trial guide referred to example files and `docs/spitdag.md` that the sandbox did not contain. Participants repeatedly tried to use those references. Their reports document this packaging gap, but it limits any conclusion that the new worked examples themselves were ineffective.

## Results

All **18 of 18** plans matched their answer keys exactly. Both diagnostic-scenario participants also named all three broken stores and their root causes. All participants rated confidence 5/5.

| Scenario | Jobs | Round 2 `spit` calls, a / b | Round 1 calls, a / b |
| --- | ---: | ---: | ---: |
| s1 logs | 24 | 6 / 4 | 9 / 8 |
| s2 cohort | 41 | 4 / 4 | 8 / 6 |
| s2 follow-up | 60 | 3 / 2 | 11 / 18 |
| s3 sweep | 41 | 5 / 4 | 10 / 10 |
| s3 one board (new) | 39 | 6 / 4 | — |
| s4 sensors | 19 | 5 / 6 | 8 / 9 |
| s4 vague brief (new) | 19 | 7 / 6 | — |
| s5 survey | 20 | 3 / 4 | 8 / 5 |
| s6 diagnose | 22 | 8 / 10 | 11 / 20 |

The 12 runs on the original six scenarios used 63 `spit` calls, versus 112 reported in round 1. The cohort change request took five additional calls across both participants, versus 29 in round 1. Both participants said the change was much faster than the first build, but neither recorded precise wall-clock time; call counts are the quantitative measure here. In the new variants, four runs used 23 calls and three calls failed before the participants corrected their plans.

## What participants found

1. **The cohort exclusion was easier.** Both follow-up plans came from a recipe using `exclude bold[sub=02,ses=02,run=3]`, kept the raw file in place, and matched all 60 jobs. Adding the new subject required no pipeline edit. Each participant added eight QC jobs and confirmed that the excluded run reached neither QC nor the session average. See the [cohort follow-up archives](results.zip) and [second replicate](results.zip).
2. **Ragged sweeps still take reasoning.** One original-sweep and one single-leaderboard participant first broadcast seed values in a way that invented a `warmup` seed 33; SPIT's missing-input error identified the bad combination. Participants asked for an inline example showing the driving input, when `@ each` keeps correlated config and seed values together, and how an explicit output dimension order affects the final collection. Both new leaderboard plans ultimately made the correct 39 jobs, including one model-then-config ordered board.
3. **The vague brief exposed a type-name mistake.** Both participants on `s4-vague` initially put lowercase product names in operation type positions. The checker reported that each type must start with a capital letter; both corrected the pipeline. Neither participant on the more explicit `s4-sensors` brief hit that error. All four plans matched 19 jobs.
4. **The diagnostic messages located the broken stores.** Both `s6-diagnose` participants identified the one-week `s03`, case-mismatched `S07` price file for `s07`, and absent `s09` price file. Both excluded the bad stores and produced the 22-job key. They spent the most thought on excluding the uppercase orphan price artifact separately from the lowercase store group.
5. **The guide package hid its own examples.** Reports across the sweep, sensor, cohort, and survey tasks requested complete examples combining several selectors, stages, outputs, and verification. The new example files and `.spitdag` reference were linked but not present in the sandbox. A future study package should either include those files or make the relevant examples self-contained in `GUIDE.md`.

The observed failed calls were mostly useful checks: two missing-seed errors, two lowercase-type checks, a `--commands`/`-o` option conflict, and one expected diagnostic failure in s6. Participants also requested shorter job counts by operation and more detail on unmatched or unused source files. These are follow-up ideas, not prerequisites to read the result.

The [action plan](../../design/round2-actions.md) orders the documentation, guide packaging, message, and follow-up study work prompted by these findings.
