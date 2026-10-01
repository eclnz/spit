# Usability study: third round after Phase 5

This round tested the settled language from Phase 5 (`usability/design/one-way.md`) with fresh, isolated participants. Two participants each attempted the ragged sweep, single-board sweep, vague sensor brief, diagnosis, and cohort. The two cohort participants then received the change request in their existing sessions. The [run archives](results.zip) hold the selected plans, pipelines, recipes, participant reports, and grading results. The [earlier pilot](../2-pilot/README.md) used the previous language and is separate from these results.

## Method and limits

Each participant received a fresh trial folder made by `harness/make_run.sh`, containing the task, dataset, logging `bin/spit` wrapper, offline guide, linked examples, and report template. All selected runs used the release binary built after Phase 5 and the same model. Participants were told to stay in their trial folder. Two initial single-board runs and one initial cohort run reported opening a platform setup skill outside the trial; their plans were excluded from the selected set and replaced with fresh runs. The selected participants reported reading only trial materials, but participant tool transcripts were unavailable, so external access cannot be audited independently. Each archived result records that limit.

The grader compares each plan's expanded job and verification commands with the reference key. It does not run the processing programs or judge whether they would handle real data. The wrapper logs `spit` calls, not shell or document reads; no comparable wall-clock times were measured.

## Results

All **12 selected plans matched their keys**: ten initial plans and two cohort follow-ups. Both diagnosis participants also named all three broken stores and their root causes. All selected participants reported confidence 5/5.

| Task | Jobs per plan | `spit` calls, two participants | Failed calls |
| --- | ---: | ---: | ---: |
| Cohort initial | 41 | 4 / 4 | 0 / 0 |
| Cohort follow-up | 60 | 3 / 4 | 0 / 0 |
| Ragged sweep | 41 | 4 / 4 | 0 / 0 |
| Single board | 39 | 3 / 3 | 0 / 0 |
| Vague sensors | 19 | 5 / 4 | 1 / 0 |
| Diagnose stores | 22 | 6 / 4 | 0 / 0 |

The one failed call was a sensor pipeline check after a participant wrote `->` with no output type. The checker said `expected type name`; the participant corrected the signature. Both cohort follow-ups discovered the new subject, added eight QC jobs, and excluded the damaged run through the recipe. Their plans retain the raw file and contain the expected 60 jobs.

## What participants wrote

1. **Sweep structure converged.** All four sweep participants wrote `dimensions [model, config, seed]`, let observed `[config, seed]` pairs drive training, used `model @ each(model)`, and placed `@ vary` on aggregate calls. Both single-board plans collected summaries in model-then-config order. Product and port names differed, but the structural choices matched. The nearby [worked example](../../../docs/examples.md#ragged-sweep-correlated-seeds-and-collection-order) helped; these runs show it is findable and adaptable, not that participants would discover the syntax without it.
2. **The cohort pipeline and recipe split was consistent.** Both selected participants kept source and operation definitions in a pipeline and put session discovery, subject removal, and the follow-up's damaged-run exclusion in a recipe. Adding the subject needed no rule change. The biggest first-build friction was repeated BIDS paths and choosing `--root data`, not the new aggregation syntax.
3. **The sensor join was still the main reasoning step.** Both used `where(revision=3)` to select the approved calibration and `same(station)` to join a baseline whose recording date did not match the reading day. One typed the roles distinctly (`Reading`, `Calibration`, `Baseline`, `Site`); the other left the pipeline untyped. The participant who tried an empty return type wanted an example of an entirely untyped operation.
4. **Types did not converge on a useful policy.** In the selected cohort plans, one participant used `Image` for both BOLD and T1w inputs, which would not catch their accidental swap; the other left input ports untyped while declaring some output types. All plans passed, so matching the task key did not test whether type declarations prevented wrong connections. A future typing example should use distinct types for inputs whose order matters, and show a rejected swap.
5. **Diagnosis stayed clear, with an orphan source to remove.** Both participants found `s03`'s one-week minimum failure, the `s07`/`S07` case mismatch, and missing `s09` pricing. Both excluded the lowercase bad store group and the separate uppercase pricing source. The case-mismatch hint was praised; participants still requested one short example of the paired exclusions. Small unmatched-file counts were distracting when they referred to recipe or pipeline files.
6. **Compact job counts remain the strongest inspection request.** Participants across the sweeps, sensor task, and cohort asked to see counts by operation without scanning a long command listing or parsing JSON. `--commands` remained useful for verifying the exact collection order. Several reports asked for a short recipe-root example beside the first command, and for a troubleshooting example that goes from `artifacts` to exclusions.

## Decisions and next work

- Add an optional `dag --counts` text view, in the order specified by the inspection design (`usability/design/round3-inspection.md`). The request persisted with the repaired guide and new language. Keep `--commands` for exact argument order.
- Add a short untyped operation signature and a `--root data` recipe invocation to the quick guide. In `docs/examples.md`, add a diagnosis walkthrough with the lowercase store exclusion and uppercase orphan pricing exclusion. Show a matched-but-unused source separately from a file reported by `inputs --unmatched`.
- Keep the current pipeline/recipe split. The sensor participants both put the approved calibration revision in a pipeline, but this study did not ask them to change revisions for a new dataset. Test such a change request before adding a recipe selection rule; see decision 3 (`usability/design/one-way.md`).
- Keep optional types and positional port type checking. Recommend distinct types where a wrong connection would otherwise pass; do not treat a passing DAG as evidence that its types protect the pipeline. See decision 4 (`usability/design/one-way.md`).
