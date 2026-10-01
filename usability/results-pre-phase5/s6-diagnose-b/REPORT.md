# Report

## 1. Outcome
Did you produce the deliverables? Confidence (1–5) that they are exactly right, and why.

Yes: `ANSWER.md`, `data/healthy.spitin`, and `plan.spitdag`. Confidence: 5/5. The successful DAG has 22 jobs: nine clean, nine price, three store reports, and one chain summary. I checked every resolved command with `spit dag --commands`; it uses only s01, s02, and s05.

## 2. Walkthrough
Your attempts in order. For every `spit` error or surprise: the message (quoted), what you thought it meant, and what you did next.

I read `TASK.md`, `GUIDE.md`, the pipeline, and the original recipe. I ran `spit artifacts data/weekly.spitin`, which identified s03's one-week minimum failure, s07's case mismatch, and s09's absent pricing input. It also said, "note: 2 files under `data` match no source rule". I ran `spit inputs data/weekly.spitin --unmatched`; the two were `pipeline.spit` and `weekly.spitin`, so they were harmless. I wrote `data/healthy.spitin` to exclude s03, s07, s09, and the separate mis-cased S07 source identity. `spit check data/healthy.spitin` reported "Recipe valid." Then `spit dag data/healthy.spitin -o plan.spitdag` reported "22 jobs resolved." I ran `spit dag data/healthy.spitin --commands` and checked the output paths and chain-summary membership. It noted three unmatched files after adding my recipe; those are the two original text files plus `healthy.spitin`.

## 3. Stuck points
Where did you stall longest, and what finally got you past it?

The s07/S07 distinction took the most thought. Excluding `[store=s07]` leaves the `S07` pricing artifact because its store value is different. I added a separate `[store=S07]` exclusion so the plan is confined to the remaining stores. The guide's explanation that values are compared as written confirmed this.

## 4. Guesses
What did you do that the guide did not confirm? Did any guess turn out wrong?

I initially wondered whether `--partial` alone would satisfy the request. The guide confirms that it can plan complete members of an aggregate, but it can also retain jobs for failed stores and record their outputs as left out. Explicit store exclusions fit the requested run better. No guess used in the final recipe turned out wrong.

## 5. Guide gaps
What did you look for in GUIDE.md and not find? Name the section you checked.

The "Find incomplete artifacts" section explained diagnosis well. The "Exclude named artifacts" section explained exact values. I did not find a concise example for excluding a bad entity whose files have two spellings, or a direct comparison of the jobs kept by `--partial` versus whole-group exclusion.

## 6. Error messages
Messages that misled you or didn't help, and any that were especially good. Quote them.

The most helpful message was "pricing[store=S07] exists; its `store` differs only in letter case". The s03 message, "input `weeks` of `store_report` needs at least 2 artifacts at [store=s03], found 1", was also clear. The unmatched-file note was mildly distracting until `--unmatched` showed they were recipe and pipeline files. I encountered no SPIT errors.

## 7. Language friction
Anything you wanted to express that the language made awkward or impossible, and the workaround you used.

The four explicit exclusions repeat the business decision in the recipe; the extra `S07` spelling is easy to overlook. I kept both exclusions with comments explaining why. I did not need to change any command or data file.

## 8. Compared with a shell script, Make, or Snakemake
Was SPIT faster, slower, or about the same to reach a correct result here? Why?

SPIT was faster for diagnosis: `artifacts` displayed every failed downstream report and its root input problem in one call. A shell script would need custom checks for the minimum week count, pricing joins, and chain membership. The recipe syntax took a little time to understand.

## 9. Top three changes
The three changes to SPIT or its guide that would have helped you most.

1. Show a worked example of diagnosing several failed groups and excluding them for one run.
2. Show `--partial` and explicit exclusions side by side, including which early jobs remain.
3. Add a recipe example that handles an entity with differently cased source identities.
