# Report

## 1. Outcome

Yes. I produced `ANSWER.md`, `data/healthy.spitin`, and `plan.spitdag` using `spit dag data/healthy.spitin -o plan.spitdag`. Confidence: 5/5. The DAG has 22 jobs: nine `clean`, nine `price`, three `store_report`, and one `chain_summary`. Its summary takes exactly the s01, s02, and s05 reports.

## 2. Walkthrough

I read `TASK.md` and `GUIDE.md`, then inspected the supplied pipeline and recipe. I ran `spit check data/weekly.spitin`, which reported `Recipe valid.` I ran `spit inputs data/weekly.spitin -o weekly.spitout`, then `spit artifacts data/weekly.spitin`. The artifact listing showed s03's minimum-week failure, s07's pricing case mismatch, and s09's missing pricing. I checked `spit inputs data/weekly.spitin --unmatched` because the first run said `2 files under data match no source rule`; they were `pipeline.spit` and `weekly.spitin`, so this was expected. I wrote a new recipe excluding the three problem stores and the orphan `S07` pricing artifact, checked it, and wrote the DAG. The new recipe reported `3 files under data match no source rule` because it adds another recipe file; no data was missing from the scan. Finally, I inspected the DAG's job count, first job, and chain summary.

The most useful diagnostic was: `pricing[store=S07] exists; its store differs only in letter case`. I took it to mean the uppercase file could not join to s07 sales; I excluded both the s07 group and the orphan pricing artifact.

## 3. Stuck points

No long stall. The main decision was whether to exclude `pricing[store=S07]` separately. The store-level `s07` exclusion would leave that differently cased artifact in the inventory, so I added a source-specific exclusion.

## 4. Guesses

I inferred that a clean plan should exclude the orphan `S07` pricing artifact, though the task only explicitly says to leave out problem stores. The successful DAG and its three store reports supported that choice. No guess proved wrong.

## 5. Guide gaps

I did not find a substantive gap for this task. The `Find incomplete artifacts` and `Exclude named artifacts` sections gave the commands and exclusion syntax I needed.

## 6. Error messages

There were no SPIT errors. The case hint quoted above was especially useful. The `2 files under data match no source rule` note initially invited inspection; `--unmatched` made it easy to see they were the pipeline and recipe, not a dataset problem.

## 7. Language friction

Leaving out the bad store plus its mis-cased pricing file took two `exclude` rules, because `S07` is a distinct value from `s07`. That is consistent with the case-sensitive join, though a single logical store problem required two exclusions.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was faster for diagnosis. `artifacts` traced missing weekly revenue into failed store reports and the chain summary, while `dag` generated the dependency plan. A shell script would need explicit inventory and failure tracing. I did not build this scenario in Make or Snakemake, so that comparison is an estimate.

## 9. Top three changes

1. Make `artifacts` offer a compact summary grouped by final output, with root causes beneath each failed store report.
2. Label unmatched pipeline and recipe files as expected metadata, or omit them from the unmatched-file note.
3. Suggest exclusion syntax alongside case-mismatch diagnostics, including how to remove the orphan artifact.
