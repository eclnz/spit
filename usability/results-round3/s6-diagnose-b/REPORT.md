# Report

## 1. Outcome
Did you produce the deliverables? Confidence (1–5) that they are exactly right, and why.

Yes: `ANSWER.md`, `remaining.spitin`, and `plan.spitdag`. Confidence: **5/5**. The plan has 22 jobs: nine clean, nine price, three store report, and one chain summary. Its only target is `reports/chain.pdf`, and `left_out` is empty. The input files were verified by SPIT.

## 2. Walkthrough
Your attempts in order. For every `spit` error or surprise: the message (quoted), what you thought it meant, and what you did next.

I read `TASK.md`, `GUIDE.md`, and `REPORT.md`, then inspected the dataset file names, pipeline, and recipe. `spit check data/weekly.spitin` said "Recipe valid." I ran `spit artifacts data/weekly.spitin`; it showed s03's one-week minimum failure, missing pricing for s09, and for s07 the hint "pricing[store=S07] exists; its `store` differs only in letter case." It also said "2 files under `data` match no source rule"; these are the pipeline and recipe files visible in the data listing, so I did not treat them as store inputs. I wrote `remaining.spitin` with exclusions for s03, s07, s09, and the unused uppercase S07 pricing identity. `spit check remaining.spitin` said "Recipe valid." `spit dag remaining.spitin --root data -o plan.spitdag` reported "12 source files verified" and "22 jobs resolved." I inspected the JSON job counts, target, and `left_out`.

## 3. Stuck points
Where did you stall longest, and what finally got you past it?

The main decision was how to omit **all** jobs for bad stores. The guide's `--partial` option would still plan their independent clean jobs. The recipe `exclude [store=...]` rule removes their source identities before planning, which matched the task.

## 4. Guesses
What did you do that the guide did not confirm? Did any guess turn out wrong?

I inferred that excluding `[store=S07]` as well as `[store=s07]` was needed to remove the unused, mis-cased pricing source. That worked: the resulting plan has only good-store jobs. No SPIT syntax guess failed.

## 5. Guide gaps
What did you look for in GUIDE.md and not find? Name the section you checked.

In "Exclude named artifacts" and "Find incomplete artifacts," I looked for a direct recommendation for excluding a store when one of its files has a different-case store identity. The pieces are documented, but the complete pattern needs to be inferred.

## 6. Error messages
Messages that misled you or didn't help, and any that were especially good. Quote them.

The case hint was especially good: "pricing[store=S07] exists; its `store` differs only in letter case." The `artifacts` reason for s03, "input `weeks` of `store_report` needs at least 2 artifacts at [store=s03], found 1," was also precise. The unmatched-file note did not identify the files directly, though it gave a command to list them. I saw no misleading SPIT error.

## 7. Language friction
Anything you wanted to express that the language made awkward or impossible, and the workaround you used.

The uppercase pricing identity required a separate group exclusion. I could not describe s07 and S07 with a single exact-value exclusion, so I wrote two rules. This was minor.

## 8. Compared with a shell script, Make, or Snakemake
Was SPIT faster, slower, or about the same to reach a correct result here? Why?

Faster for this diagnostic task. `artifacts` immediately traced the missing report back to missing or mismatched inputs, and the recipe produced a checked plan without writing custom matching logic. A shell script would require manually joining stores and enforcing the two-week minimum.

## 9. Top three changes
The three changes to SPIT or its guide that would have helped you most.

1. Add a short troubleshooting example that moves from `artifacts` diagnostics to an exclusion recipe for bad groups and a good-only chain summary.
2. Show the unmatched file names in the `artifacts` note when there are only a few, so the reader need not run another command to identify them.
3. Add a guide example where a case-mismatched source has to be excluded separately from the intended group.
