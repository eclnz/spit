# Report

## 1. Outcome

Yes. I left `panel.spit`, `panel.spitin`, and `plan.spitdag` in this folder. Confidence: 5/5. `spit check` accepted the recipe; `spit dag --commands --paths` showed 11 clean jobs, 4 model jobs, 4 charts, and 1 national table with the requested paths and arguments. I inspected the displayed fit checks and the numeric order of the northwest waves (1, 2, 10). The named programs do not exist here, so I did not execute the jobs.

## 2. Walkthrough

1. Read `TASK.md`, `GUIDE.md`, the report form, and the file names under `data`. There are four regions and 11 response CSVs. The additional `wave3.csv.bak` should be ignored.
2. Read the guide's sections on operations, `many`, multi-output jobs, `verify`, stages, paths, and recipes. Wrote one pipeline and a recipe that scans `data` as its root.
3. Ran `spit check panel.spitin --path-rules`. It said `Recipe valid.` and showed an explicit path for every product. There were no SPIT errors.
4. Ran `spit dag panel.spitin --root data --commands --paths`. It resolved the intended 20 jobs. The scan said `note: 1 files under .../data match no source rule`; I understood that as the backup file because the source path ends exactly in `.csv` and the file listing showed one backup. I made no change.
5. Ran `spit dag panel.spitin --root data -o plan.spitdag` to save the plan. It repeated the 20-job count.

## 3. Stuck points

The most thought went into expressing the fit as one job that collects a variable number of waves, writes two outputs, and validates the same collection first. The guide's `many`, multi-output, and `verify` examples supplied the three pieces. I then checked the expanded commands to confirm they combined correctly.

## 4. Guesses

I assumed a recipe beside the pipeline with `--root data` would scan only the data folder and that `wave{wave}.csv` would ignore `.bak`. The guide's path-matching and `--root` sections supported both, and the DAG result confirmed 11 sources plus one unmatched file. I also relied on the guide's natural ordering for digit runs to put wave10 after wave2; the expanded northwest command confirmed it.

## 5. Guide gaps

I did not find a single complete example combining stages, a multi-output aggregate, and `verify` in the guide itself. The Operations and commands section points to a model-fit example that is unavailable in this setup. I assembled the syntax from separate examples in Operations and commands and Stages.

## 6. Error messages

There were no errors. `Recipe valid.` and `note: 20 jobs resolved: 11 in ingest, 4 in model, 5 in publish.` were useful confirmation. The unmatched-file note was useful, though it gave a count rather than the filename; its suggested `--unmatched` command would identify it.

## 7. Language friction

The task's numeric wave order is implicit in a `many` collection. There is no explicit sort clause in the pipeline, so I had to know the guide's natural-order rule and inspect the expanded command. Otherwise the language expressed this task cleanly.

## 8. Compared with a shell script, Make, or Snakemake

About the same initial effort as a small Snakemake file, and faster to verify than a hand-written shell script: the resolved commands exposed ordering, paths, dependencies, and the validation hook before any job ran. A shell script would have needed loops and sorting logic; Make would have required generating the irregular per-region dependencies.

## 9. Top three changes

1. Put one complete stage-based model-fit example, including `verify` and both outputs, directly in the guide instead of linking to an absent example.
2. Let the expanded DAG output state the ordering rule beside a `many` input, or offer a concise ordering annotation in the pipeline.
3. Include the unmatched filenames in the default scan note when there are only a few, while retaining `--unmatched` for long lists.
