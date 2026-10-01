# Report

## 1. Outcome
Yes. I produced `pipeline.spit`, `dataset.spitin`, `dataset.spitout`, and `plan.spitdag`. Confidence: 5/5. SPIT resolved 19 jobs: 8 calibration jobs, 8 anomaly jobs, and 3 station reports. I inspected the printed paths and commands for all 19 jobs. They use revision 3, each station's own baseline, and day-ordered anomaly arguments. This is a plan only; the named programs are unavailable, so I did not execute the jobs.

## 2. Walkthrough
I read the task, the guide, CLI help, and the data filenames. I wrote a pipeline with four sources: raw readings, calibration revisions, station baselines, and site metadata. I used `where(revision=3)` for calibration, `same(station)` to select each station's single baseline despite its distinct date, and `vary(day)`/`drop(day)` to collect anomaly files for a report. I wrote a recipe pointing to the pipeline, then ran `spit check` with path rules. It reported `Recipe valid.` I ran `spit inputs` with the data root, which found 20 source artifacts. I ran `spit dag --commands --paths`, checked every command and output path, and wrote `plan.spitdag` using `spit dag ... -o`. There were no SPIT errors. The note `3 source artifacts are used by no job (calibration: 3)` was expected: these were revisions 1 and 2.

## 3. Stuck points
The main pause was choosing the join for baselines whose dates differ by station. The guide's selector section showed `same(station)` for a per-group reference, which resolved it.

## 4. Guesses
I inferred that putting `site` before the `many` anomaly input in the report operation would still form reports by station and that the `many` placeholder would expand in day order. The guide says a `many` input may sit beside `one` inputs and describes natural ordering. The printed DAG confirmed both choices. No guess proved wrong.

## 5. Guide gaps
I checked “Selectors” and “Operations and commands.” They explain the behavior needed here. The linked example files are unavailable in this trial; an inline complete example combining `where`, `same`, and a `many` aggregation would have sped up the first draft. The guide also does not give a quick way to assert expected job counts and exact command arguments from a generated DAG.

## 6. Error messages
There were no error messages. `Recipe valid.` was clear. The note `3 source artifacts are used by no job (calibration: 3)` was useful, but I had to confirm the three were the older revisions because the note gives only a count.

## 7. Language friction
A dated baseline has an extra dimension that is irrelevant to the join. Expressing that required `@ same(station)` instead of an ordinary source reference. The revision filter and report aggregation were concise once I found their syntax.

## 8. Compared with a shell script, Make, or Snakemake
SPIT took somewhat longer than a short shell script for this small fixed inventory because I had to learn selectors and write a separate recipe. Its resolved DAG made the joins, report ordering, output paths, and dependencies easy to inspect, and the same pipeline should adapt to newly added days.

## 9. Top three changes
1. Put one complete, inline pipeline and recipe in the guide that combines a fixed revision, a per-group reference with an extra dimension, and an aggregate report.
2. Let `dag` show the identities of unused source artifacts alongside its unused-source count, or point more directly to the relevant part of `artifacts` output.
3. Provide a concise DAG validation or summary mode that shows per-operation job counts and all expanded commands without the longer artifact listing.
