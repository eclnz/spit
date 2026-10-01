# Report

## 1. Outcome
Did you produce the deliverables? Confidence (1–5) that they are exactly right, and why.

Yes. I produced `stations.spit`, `stations.spitin`, and `plan.spitdag` using `spit dag stations.spitin --root data -o plan.spitdag`. Confidence: 5/5. SPIT resolved 19 jobs: eight calibrations, eight anomaly calculations, and three station reports. Its `--commands` output showed the requested paths and command forms, revision 3 in every calibration job, and each report's anomaly inputs in day order.

## 2. Walkthrough
Your attempts in order. For every `spit` error or surprise: the message (quoted), what you thought it meant, and what you did next.

I read TASK.md and GUIDE.md, then listed the data files. I saw eight daily readings, three dated baselines, three site files, and six calibration files across three stations. I wrote a pipeline with `where(revision=3)` for calibration, `same(station)` for the dated baseline, and `vary(day)` for report aggregation, plus a one-line recipe. `spit check stations.spitin` returned `Recipe valid.` I ran `spit inputs stations.spitin --root data` and saw all 20 archived files recognized. `spit dag stations.spitin --root data --commands` returned `19 jobs resolved.` The one surprise was `3 source artifacts are used by no job (calibration: 3)`; I took this to mean the unapproved revisions, confirmed from the displayed commands that all calibration jobs use rev3, and continued. Finally I wrote the DAG with `-o`.

There were no SPIT errors or failed attempts.

## 3. Stuck points
Where did you stall longest, and what finally got you past it?

The baseline's recorded date is unrelated to each reading's day, so the join needed thought. The `Selectors narrow what an input matches` part of GUIDE.md showed that `same(station)` ignores the other dimension while requiring exactly one matching reference per job. The dataset has one baseline per station.

## 4. Guesses
What did you do that the guide did not confirm? Did any guess turn out wrong?

I used the archive's `rev3.json` filenames to infer that revision values should be `3`, and used the baseline filename as a separate `recorded` dimension. These were dataset interpretations, not undocumented syntax. The discovered input list and command preview supported both. I made no guess that turned out wrong.

## 5. Guide gaps
What did you look for in GUIDE.md and not find? Name the section you checked.

I found the needed syntax in `Products and dimensions`, `Operations and commands`, `Paths`, `Recipes`, and `Where files live`. I did not find a gap that blocked this task. A single example combining `where`, `same`, and `vary` over files with different date dimensions would have made the baseline join quicker to recognize.

## 6. Error messages
Messages that misled you or didn't help, and any that were especially good. Quote them.

There were no error messages. `Recipe valid.` was a useful first check. `3 source artifacts are used by no job (calibration: 3); \`spit artifacts\` lists them` was useful because it flagged the deliberately unused revisions. `19 jobs resolved.` made the expected 8 + 8 + 3 count easy to verify.

## 7. Language friction
Anything you wanted to express that the language made awkward or impossible, and the workaround you used.

The station report had to gather a variable number of days while retaining station identity. `anomalies @ vary(day)` handled that directly. The dated baseline was mildly awkward because its date needed an identity dimension even though processing ignores it; `baseline @ same(station)` was the workaround. Nothing appeared impossible to express.

## 8. Compared with a shell script, Make, or Snakemake
Was SPIT faster, slower, or about the same to reach a correct result here? Why?

About the same as a short shell or Snakemake workflow for this small archive. Writing the selectors took some guide reading, but SPIT then discovered all files, checked the joins, and displayed every concrete command. For a changing archive, the same pipeline should save work because it does not list stations or days.

## 9. Top three changes
The three changes to SPIT or its guide that would have helped you most.

1. Add a compact example where a per-station reference has an unrelated date dimension and is joined with `same(station)`.
2. Add an end-to-end recipe example that uses `--root` when pipeline and recipe files sit outside the data folder.
3. Show the expected job count and unused-source note alongside a `where` selector example, so deliberately unused revisions are immediately recognizable.
