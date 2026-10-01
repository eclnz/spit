# Report

## 1. Outcome

Yes. I wrote `rest.spit`, `rest.spitin`, `rest.spitout`, and `plan.spitdag`. Confidence: **5/5**. SPIT resolved 41 jobs: 13 motion corrections, 6 brain extractions, 13 registrations, 6 session averages, and 3 longitudinal jobs. The `--commands` view showed the requested command argument order, output paths, numeric run order, and session order. The input scan reported that sub-03 was dropped for having one session.

## 2. Walkthrough

I read `TASK.md`, `GUIDE.md`, and this report form, then searched within the guide for the `many`, `vary`, `drop`, command, path, discovery, and ordering rules. I wrote the pipeline with seven explicit path rules and a recipe with directory discovery and the subject-level drop rule. `bin/spit check rest.spitin --path-rules` returned “Recipe valid.” I ran `bin/spit inputs rest.spitin --root data -o rest.spitout`; it reported “note: dropped [sub=03] by `drop [sub] where sessions count<2` (line 4); found 1” and “note: found 19 source artifacts and 6 contexts under `data`.” I then ran `bin/spit dag rest.spitin --root data --commands` to review all commands, followed by `bin/spit dag rest.spitin --root data -o plan.spitdag`.

There were no SPIT errors. The only initial surprise was “note: 24 files under `data` match no source rule”; I understood these to include sidecars and top-level files that the task explicitly excludes, so I continued. I did not enumerate them individually.

## 3. Stuck points

The longest part was translating the repeated BIDS path names into explicit path rules and checking that every output retained the input's zero-padded subject and session text. The guide's path examples and the `--commands` preview resolved this.

## 4. Guesses

I used `--root data` so the path rules would start at `sub-...` while outputs would start at `derivatives/...`. The guide describes this, so it was a reasoned choice rather than an unsupported guess. I initially relied on the documented natural ordering of `many` inputs for runs and sessions; the command preview confirmed it. No guess turned out wrong.

## 5. Guide gaps

I checked “Operations and commands,” “Where files live,” and the recipe discovery/drop sections. I found the needed rules there. A single end-to-end example that combines directory discovery, a cohort drop, two levels of aggregation, and command preview would have reduced the amount of cross-reading, but I did not find a missing rule that blocked the task.

## 6. Error messages

There were no errors. “Recipe valid.” was useful after writing the pipeline and recipe. “note: dropped [sub=03] by `drop [sub] where sessions count<2` (line 4); found 1” was especially useful because it directly verified the study rule. The “24 files ... match no source rule” note was initially surprising but explained by the task's non-input files.

## 7. Language friction

The pipeline expresses the work cleanly. Writing seven similar BIDS path rules was repetitive and easy to mistype. I used explicit rules and checked the resolved commands. The separate recipe and pipeline files added a small amount of setup for this one dataset, while making the cohort rule clear.

## 8. Compared with a shell script, Make, or Snakemake

For this small dataset, SPIT was probably about the same speed as a concise Snakemake file and slower than a quick shell loop to draft. It was faster to verify the cohort exclusion and every generated command because the planner printed all jobs before anything ran. A shell loop would need separate checking to establish the same confidence.

## 9. Top three changes

1. Add a compact worked example with directory discovery, subject-level dropping, and two nested `many` aggregations.
2. Show a per-step job count summary in `dag` output, so a user can quickly compare expected and actual counts.
3. Add a path-rule preview or template helper for repetitive layouts such as BIDS, where the same dimensions appear several times in each path.

## Change request

1. **New sub-06:** I changed no pipeline or recipe rule. Rescanning the directory added its two sessions and four BOLD runs automatically. This took seconds of editing time, compared with several minutes to build the first pipeline. The new subject's jobs were easy to recognize in `--commands` output; scanning the longer output still took a little time.
2. **Per-session QC:** I added one output path rule, a `report` operation with one brain input and a `many` coregistered-run input, its command, and one step. This took roughly a minute, much less than the first build. The mixed single and `many` input syntax required a quick check against the guide I had already read; the command preview made the resulting join and run order clear.
3. **Corrupted sub-02 ses-02 run 3:** I added `exclude bold[sub=02,ses=02,run=3]` to the recipe. This took seconds, much less than the first build. The input scan explicitly reported the exclusion, and the command preview showed no processing job for run 3 and only runs 1 and 2 in that session's average and QC report. I did not alter the raw file. It was straightforward, though verifying the absence of a job in a 60-job listing was mildly cumbersome.

I refreshed `rest.spitout` and overwrote `plan.spitdag`. `spit check` reported “Recipe valid.” and `spit dag` resolved 60 jobs: 16 motion corrections, 8 brain extractions, 16 registrations, 8 averages, 8 QC reports, and 4 longitudinal jobs. The scan continued to drop sub-03.
