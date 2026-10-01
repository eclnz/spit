# Report

## 1. Outcome

Yes. I wrote `archive.spit`, `archive.spitin`, and `plan.spitdag` using `spit dag archive.spitin --root data -o plan.spitdag`. Confidence: 5/5. The displayed plan has 8 calibration jobs, 8 anomaly jobs, and 3 station reports. I checked every command line: all calibrations use revision 3, each anomaly uses its station's sole baseline, and each report lists available days in ascending order. I did not run the absent processing programs.

## 2. Walkthrough

I read the task, guide, and report template, then listed the archive. I found three stations, eight readings (east lacks June 2), one baseline and one site file per station, and three old calibration files. I looked up `where`, `same`, `many`, `drop`, command placeholders, path rules, and recipe roots in the guide. I wrote the pipeline and recipe, ran `spit check archive.spitin --path-rules`, ran `spit dag archive.spitin --root data --commands`, then wrote `plan.spitdag` with `-o`.

No `spit` command returned an error. The only surprise was `note: 3 source artifacts are used by no job (calibration: 3); \`spit artifacts\` lists them`. I took this to mean the three nonapproved revisions were found but skipped by `where(revision=3)`. The commands confirmed it, so I kept the pipeline.

## 3. Stuck points

Choosing the join for the dated baseline took the most thought. Its file path supplies both `station` and a recording date, while a daily reading has its own, unrelated `day`. The guide's `same(station)` example made the intended join clear.

## 4. Guesses

I assumed the phrase "one baseline per station" remains true for future stations, so `same(station)` will yield exactly one baseline. I also assumed new stations come with a site file and revision 3 calibration. The guide says these inputs must resolve, but cannot guarantee future archive completeness. No guess proved wrong in this dataset.

## 5. Guide gaps

In **Selectors narrow what an input matches**, I wanted a single worked example combining `where`, `same`, and `many` with an archive whose baseline has a date unrelated to the reading date. The individual rules are documented; connecting them still required careful interpretation. In **Where files live**, the recipe's default root versus `--root data` took a reread.

## 6. Error messages

There were no errors. `Recipe valid.` and `note: 19 jobs resolved.` were helpful. The unused source note quoted above was useful, though it did not itself say those three were the intended older revisions; `--commands` supplied that evidence.

## 7. Language friction

The archive's baseline recording date is irrelevant to matching yet required a declared dimension to extract its path. `baseline @ same(station)` expressed the join, but only because there is exactly one baseline per station. Otherwise that selector would be ambiguous. Selecting approved calibration via `where(revision=3)` was straightforward.

## 8. Compared with a shell script, Make, or Snakemake

For this small archive, a shell script might have been faster to draft. SPIT took more guide reading, but it resolved the sparse days, revision filter, baseline join, and ordered report inputs without hand-listing files. Its printed commands made the result easy to inspect.

## 9. Top three changes

1. Add a compact example that combines a filtered revision, a station-only join to a dated reference, and a day-ordered `many` report.
2. Make the unused source note identify selector filtering when possible, so intentional old revisions are easier to distinguish from mistakes.
3. Give a short recipe-root example near the basic `spit dag` invocation, showing a pipeline and recipe beside one another with `--root data`.
