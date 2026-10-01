# Report

## 1. Outcome

Yes. I wrote `analysis.spit`, `cohort.spitin`, `cohort.spitout`, and `plan.spitdag`. Confidence: 5/5. `spit dag` resolved 41 jobs: 13 motion corrections, 6 brain extractions, 13 coregistrations, 6 session averages, and 3 longitudinal combinations. Its command listing matched the requested paths and argument order. The plan dropped `sub-03` entirely because it has one session.

## 2. Walkthrough

I read `TASK.md`, `GUIDE.md`, and the report template, then listed the filenames under `data/`. I found seven session folders across four subjects; `sub-03` has only one. I wrote a pipeline with two source families, five operations, their commands, and explicit output path rules. I wrote a recipe that discovers session directories, drops subjects with fewer than two sessions, and maps the BIDS source paths. `spit check cohort.spitin --path-rules` reported `Recipe valid.` with all seven products covered. `spit inputs cohort.spitin --root data -o cohort.spitout` reported "note: dropped [sub=03] by `drop [sub] where sessions count<2` (line 4); found 1" and "note: found 19 source artifacts and 6 contexts under `data`". `spit dag cohort.spitin --root data -o plan.spitdag` reported `note: 19 source files verified.` and `note: 41 jobs resolved.` I then used `spit dag cohort.spitin --root data --commands` to inspect all expanded commands.

There were no SPIT errors. The only initially surprising message was "note: 24 files under `data` match no source rule; `spit inputs cohort.spitin --unmatched` lists them". I understood this as the JSON sidecars and two top-level files, which the task explicitly says to ignore, so I continued.

## 3. Stuck points

The longest part was translating the repeated BIDS subject and session labels into path templates and keeping the dataset root straight. The guide's path and recipe sections, followed by `check --path-rules` and the expanded commands, settled this.

## 4. Guesses

I inferred that a path template may repeat `{sub}` and `{ses}` within one filename, and that passing `--root data` would make both discovered directories and source/output paths relative to `data/`. The guide explained roots and placeholders generally but did not show this BIDS shape. Both guesses worked in `check`, `inputs`, and `dag`. No guess turned out wrong.

## 5. Guide gaps

In **Paths** and **Recipes**, I looked for an example with repeated entity placeholders in one path and a recipe outside the dataset root. I found the general rules but no compact example combining both. I did not find any blocking gap.

## 6. Error messages

There were no errors. The exclusion note, "note: dropped [sub=03] by `drop [sub] where sessions count<2` (line 4); found 1", was especially useful because it named the excluded subject and the rule. The `24 files ... match no source rule` note initially required me to reconcile it with the expected sidecars, but its suggested `--unmatched` command was clear.

## 7. Language friction

The exact BIDS output paths required five long, repetitive `path product:` declarations. This was manageable, but there was no apparent way to share the `derivatives/sub-{sub}/ses-{ses}/...` prefix while still producing each required filename. I used explicit paths so the command listing was easy to audit.

## 8. Compared with a shell script, Make, or Snakemake

About the same time as a short script for this fixed dataset. SPIT took some time to learn and required verbose path declarations, but its discovery, subject-level exclusion, numeric ordering, and complete 41-job inspection saved manual loop and dependency checks. It would likely be faster to reuse when sessions or runs change.

## 9. Top three changes

1. Add a small BIDS-style example showing repeated placeholders and `--root` with a recipe beside the pipeline.
2. Allow a reusable path prefix or another concise way to avoid repeating long derivative path segments.
3. When inputs report unmatched files, include a short breakdown by suffix or directory so sidecars are easier to recognize without another command.
