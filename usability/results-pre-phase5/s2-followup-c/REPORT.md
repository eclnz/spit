# Report

## 1. Outcome
Produced `study.spit`, `cohort.spitin`, `cohort.spitout`, and `plan.spitdag`. Confidence: 5/5. SPIT resolved 41 jobs: 13 motion correction, 6 brain extraction, 13 coregistration, 6 session average, and 3 longitudinal jobs. The command listing shows the requested command forms and output paths, ordered runs and sessions, and no job for excluded `sub-03`.

## 2. Walkthrough
I read `TASK.md` and `GUIDE.md`, especially the pipeline, operation, recipe, discovery, and drop sections. I also read the `spit help` output. I wrote the pipeline with one source each for BOLD and T1w and five operations, plus a recipe that discovers session directories and drops subjects with fewer than two. `spit check cohort.spitin` said `Recipe valid.` I built an input inventory with `spit inputs cohort.spitin --root data -o cohort.spitout`. It reported 19 source artifacts and six contexts, and `note: dropped [sub=03] by `drop [sub] where sessions count<2` (line 4); found 1`. I then ran `spit dag cohort.spitin --root data -o plan.spitdag` and inspected the full command listing with `spit dag cohort.spitin --root data --commands`. There were no errors. The input scan also said `note: 24 files under `data` match no source rule; `spit inputs cohort.spitin --unmatched` lists them`. I took these as the sidecars and top-level files the task explicitly excludes; I did not need to list them individually.

## 3. Stuck points
Choosing the dataset root took the most thought. The recipe is in the working folder while its source and discovery rules should be relative to `data/`. The guide's “Where files live” section resolved this: use `--root data` when scanning and planning.

## 4. Guesses
I inferred that `--root data` would also make every derivative path relative to `data/`. The guide confirms this. I assumed two session averages per retained subject, and verified the command listing. No guess turned out wrong.

## 5. Guide gaps
I found no material gap for this task. I checked “Where files live,” “Operations and commands,” “Recipes,” “Discover contexts from directories,” and “Drop groups that fail a criterion.”

## 6. Error messages
There were no errors. The drop note was especially useful because it named the excluded subject, the rule, and the observed session count. The note about 24 unmatched files was informative but did not identify them; `--unmatched` is documented for that purpose.

## 7. Language friction
Keeping dataset layout in a recipe while leaving the operation graph in the pipeline required two files, but the separation was manageable. I had to repeat the full BIDS path pattern for the two source types and each derivative output.

## 8. Compared with a shell script, Make, or Snakemake
For this small fixed dataset, a shell script might have been quicker to type. SPIT was about as fast to get a checked plan because its path rules, automatic joins, numeric collection order, and subject drop handled the irregular run count and exclusion without custom loops.

## 9. Top three changes
1. Add a compact end-to-end BIDS example with `--root`, discovery, dropping, and two levels of `many` aggregation.
2. Show an optional count of jobs by operation in the `dag` summary.
3. Summarize unmatched files by extension or folder in the `inputs` note, so users can tell expected sidecars from unexpected inputs without a second command.

## Change request

1. **New `sub-06`:** I changed no pipeline or recipe rule. I rescanned the dataset and regenerated the DAG with `--root data`; discovery picked up both sessions and their four BOLD runs. This part took seconds, much less time than the first build. The only extra work was checking the new subject's commands.
2. **Per-session QC:** I added a `qc_report` operation with a `many` coregistered-run input and a single same-session brain input, its `qcreport` command, and the requested HTML path. It produced eight QC jobs, one per retained session. This took a few minutes, less than the first build because the existing average operation supplied a pattern. Placing the `many` input beside the one brain input was the main point to think through; the guide covered it.
3. **Corrupted run:** I added `exclude bold[sub=02,ses=02,run=3]` to the recipe, leaving the raw file untouched. The input scan reported the exclusion, and the command listing shows no processing for run 3 and only runs 1 and 2 in that session's average and QC report. This took less than a minute, much less than the first build. Nothing was harder than expected.

The regenerated plan contains 60 jobs: 16 motion corrections, 8 brain extractions, 16 coregistrations, 8 session averages, 8 QC reports, and 4 longitudinal combinations. `sub-03` is still dropped for having one session.
