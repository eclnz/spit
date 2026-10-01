# Report

## 1. Outcome

Yes. I created `analysis.spit`, `cohort.spitin`, `cohort.spitout`, and `plan.spitdag`. Confidence: **5/5**. SPIT resolved 41 jobs: 13 motion corrections, 6 brain extractions, 13 coregistrations, 6 session averages, and 3 longitudinal combines. Its printed commands matched the requested paths and argument order. The saved DAG records subject 03 as dropped and has no `left_out` jobs.

## 2. Walkthrough

I read `TASK.md`, `GUIDE.md`, and the report template, then listed dataset filenames. I saw subjects 01, 02, and 04 had two sessions each; subject 03 had one. I wrote a pipeline with two sources, five operations, and explicit output paths. I wrote a recipe that discovers session directories and drops subjects with fewer than two sessions. `spit check cohort.spitin` returned `Recipe valid.` I ran `spit inputs cohort.spitin --root data -o cohort.spitout`, inspected `spit dag cohort.spitin --root data --commands`, then wrote `plan.spitdag` with `spit dag cohort.spitin --root data -o plan.spitdag`. Finally I inspected the saved DAG's job count, root, removal, and `left_out` fields.

There were no SPIT errors. The first inputs run reported “note: 24 files under `data` match no source rule; `spit inputs cohort.spitin --unmatched` lists them”. I checked that this did not indicate missed NIfTI inputs; the JSON sidecars and top-level files intentionally have no source rule. It also reported “note: dropped [sub=03] by `drop [sub] where sessions count<2` (line 4); found 1”, which confirmed the exclusion. The DAG reported “note: 41 jobs resolved.”

## 3. Stuck points

I did not get stuck. The most careful part was matching the dataset root to the path patterns: the recipe and pipeline are in the working folder, while source and output paths are relative to `data/`. The guide's “Where files live” section resolved this; I used `--root data`.

## 4. Guesses

I expected the `drop` rule to remove both source families and all jobs for subject 03. The guide's “Drop groups that fail a criterion” section supported that, and the DAG confirmed it. I made no guess that turned out wrong.

## 5. Guide gaps

I found the needed syntax in “Dimension order,” “Operations and commands,” “Where files live,” “Discover contexts from directories,” and “Drop groups that fail a criterion.” I found no blocking gap. I would have liked a short worked example combining `--root`, `discover`, `drop`, and two levels of `many` aggregation, rather than assembling those pieces from separate sections.

## 6. Error messages

There were no errors or misleading messages. “note: dropped [sub=03] by `drop [sub] where sessions count<2` (line 4); found 1” was especially useful: it named the exact excluded subject, rule, and observed count. “note: 41 jobs resolved.” was useful for checking the total. The 24 unmatched files note was initially a reason to double-check the source patterns but was accurate for the noninput files.

## 7. Language friction

No major friction. Explicit `path` lines for each product were repetitive, but necessary to preserve the exact BIDS naming requested here. The `@ vary(run)` and `@ vary(ses)` selectors expressed the two aggregation levels cleanly.

## 8. Compared with a shell script, Make, or Snakemake

About the same time as writing a short Snakemake workflow for me. SPIT needed some guide reading, but discovery, the cohort drop, natural numeric run ordering, and the command preview saved manual enumeration and made the plan easier to check. A shell script would have been quick to start but more work to validate at this level.

## 9. Top three changes

1. Add a compact example that combines directory discovery, `drop`, `--root`, and nested `many` aggregation.
2. Show a breakdown of resolved job counts by operation in the normal `dag` summary.
3. Let the unmatched-file note distinguish files whose paths match no source pattern from artifacts deliberately removed by rules, so its meaning is immediately clear.
