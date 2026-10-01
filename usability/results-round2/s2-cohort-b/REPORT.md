# Report

## 1. Outcome

Yes. I wrote `rest.spit`, `cohort.spitin`, and `plan.spitdag` with `spit dag ... -o`. Confidence: 5/5. The DAG has 41 jobs: 13 motion corrections, 6 brain extractions, 13 coregistrations, 6 session averages, and 3 longitudinal combinations. Its removal record names `sub-03`, the only subject with one session. I inspected all printed commands and their input order.

## 2. Walkthrough

I read `TASK.md`, `GUIDE.md`, `REPORT.md`, `spit help`, and the dataset filenames. I wrote a pipeline with separate rules for every source and product path and a recipe that discovers session directories and drops subjects with fewer than two sessions. `spit check cohort.spitin --path-rules` reported `Recipe valid.` I then ran `spit dag cohort.spitin --root data -o plan.spitdag`, which reported `41 jobs resolved.` and `dropped [sub=03] ... found 1`. Finally, `spit dag ... --commands` showed the expected paths, run order, and session order for every job.

There were no SPIT errors. One note was initially surprising: `24 files ... match no source rule`. I checked this against the dataset listing: 22 JSON sidecars and two top-level files are explicitly not inputs, so I did not add source rules for them. The dropped subject's scans do match source paths but are removed by the recipe.

## 3. Stuck points

The longest pause was deciding how to count sessions while excluding an entire subject. The guide's `discover sessions: [sub, ses] from dirs ...` and `drop [sub] where sessions count<2` example resolved it. The recipe uses `--root data` because its file lives beside, rather than inside, the dataset.

## 4. Guesses

I assumed that the source and output names did not need to follow BIDS names exactly, since the path rules determine filenames; the printed DAG confirmed the paths. I also used `@ min(2)` on the longitudinal operation as a second constraint. The guide describes this and the two-session subjects resolved correctly. No guess turned out wrong.

## 5. Guide gaps

I checked `Where files live`, `Discover contexts from directories`, and `Operations and commands`. I did not find a complete example combining a recipe outside the dataset, a directory discovery rule, a subject-level drop, and several dependent operations. The individual pieces were documented, but assembling them required reading several sections.

## 6. Error messages

There were no errors. `note: dropped [sub=03] by \`drop [sub] where sessions count<2\` (line 4); found 1` was especially useful because it verified both the excluded identity and the count. `note: 24 files ... match no source rule` could look alarming without knowing that sidecars and top-level files are intentionally ignored; listing which categories were unmatched in the summary would help.

## 7. Language friction

The many-input syntax required an operation contract (`@ drop(run)` or `@ drop(ses)`) and a call selector (`@ vary(run)` or `@ vary(ses)`). This was a little repetitive but worked. Specifying all seven path rules was verbose, although necessary for exact requested paths.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was slower for this small fixed dataset because I had to learn its syntax and recipe model. Once written, the recipe handled the exclusion and the pipeline produced ordered, dependency-aware jobs without hand-writing loops, so it should be faster when the cohort changes.

## 9. Top three changes

1. Add one end-to-end example with a recipe outside the dataset using `--root`, `discover`, `drop`, and a multi-step pipeline.
2. Show a compact summary of the unmatched files by extension or directory, especially when there are many sidecars.
3. Add a BIDS-like aggregation example that explicitly shows how `@ drop` and `@ vary` work together and how many-input argument order is chosen.
