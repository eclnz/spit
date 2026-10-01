# Usability pilot before Phase 5

This pilot tested the repaired offline guide and the new complete walkthroughs on the tasks most affected by the second study's documentation gaps. Two isolated participants each attempted the ragged sweep, single-board sweep, vague sensor brief, diagnosis, and cohort. Two cohort participants then handled the same change request in their existing trial sessions. These trials used the earlier language, before Phase 5 changed aggregation syntax, dimension order, and the permitted forms. They do not validate the post-Phase-5 language. The [run archives](results.zip) hold the selected plans, participant reports, and grading results.

## Method and limits

Each participant worked inside a fresh folder from `harness/make_run.sh`, with the task, dataset, logging `bin/spit` wrapper, `GUIDE.md`, linked docs and examples, and report template. They were instructed not to use the repository, other trial folders, or the web. All participants used the same model. The grader compares expanded job and verification commands against a task key; a passing grade says the plan matches the specified commands, not that the absent processing programs would run successfully.

Usage stopped an early cohort participant before a plan was made. An early diagnosis run produced a passing plan but left its report blank. Replacement participants completed those tasks, and the cohort replacement received its follow-up in the same session. The selected archives contain two passing initial plans per task and two passing follow-ups. One selected cohort initial plan has an unfilled report; its plan and wrapper log remain available. The initial plan of the other follow-up participant was graded 41/41 before it was overwritten for the change request, but that initial artifact was not retained. The archived initial plans are from the other two cohort runs.

Only `spit` wrapper calls and participant files were retained. Participant tool transcripts were unavailable, so reads outside the trial and web access could not be audited independently. The `audit.error` in each result records this limit. Call counts below exclude shell and file-reading commands; no wall-clock times were measured.

## Results

All **12 selected plans matched their keys**: ten initial plans and two cohort follow-ups. Both selected diagnosis participants identified the one-week `s03`, the `s07`/`S07` case mismatch, and the absent `s09` pricing file. Every completed participant who gave a confidence score reported 5/5.

| Task | Jobs per plan | `spit` calls, two participants | Failed calls |
| --- | ---: | ---: | ---: |
| Cohort initial | 41 | 4 / 4 | 0 / 0 |
| Cohort follow-up | 60 | 4 / 4 | 0 / 0 |
| Ragged sweep | 41 | 3 / 3 | 0 / 0 |
| Single board | 39 | 4 / 3 | 0 / 0 |
| Vague sensors | 19 | 3 / 3 | 0 / 0 |
| Diagnose stores | 22 | 5 / 6 | 0 / 1 |

The one failed diagnosis call was an initial `dag` on the damaged dataset; its case-mismatch hint led to `artifacts` and then a complete plan. The follow-up plans each include eight QC jobs, retain the new subject, and exclude the corrupted run from its session average and QC report without moving the raw file.

## What the repaired guide showed

1. **The sweep walkthrough was used.** Both ragged-sweep and both single-board participants opened `docs/examples.md`. All four planned the observed config/seed pairs without inventing an absent seed, and both single-board plans gave the leaderboard model-first input order. The explicit `[model, config]` declaration in the walkthrough was cited as the reason. Because the walkthrough is close to these tasks, these runs test whether a complete example is findable and adaptable more than they test unaided discovery of the syntax.
2. **The vague sensor brief produced no type-name error.** Both participants completed it without a failed call. They still spent the most thought on joining a dated reference by station while keeping the reading day separate. The unused-source note for archived calibration revisions prompted a check of whether those files entered the plan; `--commands` answered it.
3. **Diagnosis remained accurate, with one extra exclusion to reason through.** Both selected participants found all three bad stores. They excluded lowercase `s07` as a group and separately excluded the uppercase `S07` pricing source. One participant used `inputs --unmatched` to see that the unmatched files were recipe and pipeline files; the other found the count distracting but continued. Both requested a short example of the two spellings.
4. **The cohort change request worked twice.** The two follow-up participants added QC, let discovery pick up the new subject, and used a recipe exclusion for the corrupted run. One cited the `many` input beside a single brain input as the main design choice. Both matched all 60 jobs.
5. **Long command lists still invite a compact count.** All four sweep reports and both completed cohort reports requested or valued per-operation job counts. Several sweep participants also wanted a quick view of inferred output dimensions. `dag --commands` exposed the exact collection order, but checking operation counts required scanning its long listing or inspecting the JSON plan.

## Next decisions

- Test a compact, optional per-operation count view in the post-Phase-5 study. The repeated request survived the repaired guide; the [inspection ideas](../../design/pre-phase5-inspection.md) specify candidate text and interactions with existing output flags.
- Keep `--commands` as the exact preview of `many` argument order. The pilot used an explicit output annotation; Phase 5 replaced that with a pipeline-wide dimension order. Test whether the new rule resolves the remaining uncertainty.
- Improve the diagnosis walkthrough with the lowercase group and uppercase orphan-source exclusion together. Consider a targeted hint for a near-case unused source left after a group exclusion. Keep `inputs --unmatched` for files that match no source rule; it is a different case from a matched but unused source.
