# Report

## 1. Outcome
Yes. I produced `stations.spit`, `stations.spitin`, and `plan.spitdag`. Confidence: 5/5. `spit dag --commands` shows eight calibration jobs, eight anomaly jobs, and three report jobs with the requested paths and argument order. The written DAG has 19 jobs and contains no rev1 or rev2 path. I did not run the processing programs because they are absent.

## 2. Walkthrough
I listed the files under `data/` and found three stations, eight reading files, one baseline and site file per station, and calibration revisions 1–3. I read the guide sections on products, operations, selectors, aggregation, paths, and recipes. I wrote a pipeline with reading `[station, day]`, calibration `[station, revision]`, baseline `[station, recorded]`, and site `[station]`. I used `where(revision=3)` to select the approved calibration, `same(station)` to associate a baseline whose recorded date is unrelated to the reading day, and `many` with `drop(day)` to form each report. I wrote a recipe naming the pipeline, then ran `spit check stations.spitin`, which said `Recipe valid.` I ran `spit dag stations.spitin --root data --commands` and checked every displayed command and report input order. Finally I ran `spit dag stations.spitin --root data -o plan.spitdag` and inspected its job count and revision paths.

There were no errors. The only surprise was `note: 3 source artifacts are used by no job (calibration: 3); \`spit artifacts\` lists them`. I understood these as the archived revisions 1 and 2, confirmed neither revision path appears in the DAG, and kept them available to the scan because the `where` selector expresses the approved revision explicitly.

## 3. Stuck points
The most thought went into the baseline join: the baseline has a recorded date in its path, but that date must not be matched to a reading day. The guide's `same(station)` example resolved this.

## 4. Guesses
I inferred that `same(station)` would work even though the baseline's extra dimension is named `recorded`, and that the `many` input would sort ISO-formatted `day` values. The guide confirms both behaviors, and the generated commands confirmed the resulting bindings and order. No guess turned out wrong.

## 5. Guide gaps
I found the needed rules in `Products and dimensions`, `Operations and commands`, `Paths`, and `Recipes`. I did not need a missing feature explained. A compact example combining `where`, `same`, and `many` for this exact kind of archive would have shortened the lookup.

## 6. Error messages
There were no errors. `Recipe valid.` was clear. The unused calibration note was accurate, though it initially prompted me to check whether unused revisions entered the written plan; they did not.

## 7. Language friction
The baseline path's date needs its own dimension solely to parse the archived filename. `same(station)` handles the join, but this relationship took a moment to work out. Otherwise the command templates mapped directly to the requested forms.

## 8. Compared with a shell script, Make, or Snakemake
SPIT was slower than a short shell script to write initially because I needed to learn its syntax. Its `--commands` output made checking the irregular day sets and joins quick, and the unchanged pipeline will adapt as the folder grows. I have not timed a comparable Make or Snakemake implementation.

## 9. Top three changes
1. Add an end-to-end example with a dated reference file joined only by a shared station or subject dimension.
2. Make the unused-source note say explicitly that unused sources are omitted from the written DAG.
3. Show a small `many` example with the actual expanded command arguments, making input order visible beside the sorting rule.
