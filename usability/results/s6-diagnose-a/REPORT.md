# Report

## 1. Outcome
Yes, both are done. `ANSWER.md` is written. `plan.spitdag` was written by `spit dag data/thisweek.spitin -o plan.spitdag` from a new recipe, `data/thisweek.spitin`. It holds 22 jobs:

- 9 clean jobs and 9 price jobs for s01, s02 and s05
- 3 store reports
- 1 chain summary over those three stores

Confidence: 4/5. The jobs, commands and paths match `pipeline.spit` exactly, and `spit dag --paths` plus the JSON confirm that. The one judgement call is scope. I left out *all* jobs for the problem stores, including the s03 clean and price jobs that could technically run, because the business said problem stores are left out of the run.

## 2. Walkthrough
1. `spit check data/weekly.spitin` returned "Recipe valid.", which is fine.
2. `spit dag data/weekly.spitin` returned `error: line 18, column 26: no pricing artifact for input prices of price at [store=s07,week=2026-W36]`. That named only one store. The guide says `dag` stops at the first failure, so I switched to `artifacts`.
3. `spit artifacts data/weekly.spitin` was very good. It gave the full list of incomplete artifacts and their reasons: s07 and s09 have no pricing, s03 "needs at least 2 artifacts ... found 1", and the summary is blocked by all three. It also listed `pricing[store=S07]` as a complete source, which is how I spotted the case problem. Nothing flagged that `S07` is an orphan, though. I had to compare it with the `ls` output myself.
4. First recipe attempt: `discover stores: [store] from dirs sales/{store}`, plus `skip sales count>=2 per [store]` and `skip pricing count=1 per [store]`. `spit check` said valid. `spit inputs` failed with `error: source file pricing/S07.json for pricing lies outside the discovered contexts`. That is a clear message, but it meant a stray file I may not rename blocks the whole discovery approach. The guide does not say whether it is possible to ignore or skip a file that lies outside the contexts.
5. I dropped the `discover` line and kept only the two `skip` rules. This worked. Stores with *zero* pricing artifacts (s09, s07) still formed groups and were skipped. That surprised me: I expected `per [store]` groups to come only from the product being counted. It appears groups are drawn from all sources. The orphan `S07` group was dropped by `skip sales` (0 sales). The stderr warnings listed each skipped group and the rule that caused it, which was very helpful.
6. `spit dag data/thisweek.spitin --paths`, then `-o plan.spitdag`. I checked the JSON argv by hand.

## 3. Stuck points
The only real stall was the discovery error on `S07.json`. Removing `discover` got past it. I then had to trust, and verify through the warnings, that a `count=1` skip applies to groups where the product has zero artifacts.

## 4. Guesses
- That `skip X count=1 per [store]` evaluates a store with no X artifacts as count 0. The guide does not say how groups are formed without `discover`. It turned out right.
- That the problem stores' upstream jobs (s03 clean and price) should be excluded too. This is a reading of the task, not something SPIT decides.

## 5. Guide gaps
- "Constraints" and "Use skip ...": the guide does not say where the groups of a `per [...]` clause come from when there is no `discover` rule. Are they only from the counted product, or from every source?
- "Discover contexts from directories": the "lies outside the discovered contexts" failure is not documented, and neither is how to exclude a stray file.
- There is no way documented to exclude a specific value, such as a hypothetical `skip store=s03`. I relied on count rules.
- "Find incomplete artifacts" is good, but it does not mention orphan sources (a source artifact that no job consumes).

## 6. Error messages
- Good: the `artifacts` output, and `warning: skipped [store=s07] because skip pricing rejected the group`.
- Good but incomplete: `no pricing artifact for input prices of price at [store=s07,...]`. It would have been far more useful to add "note: pricing[store=S07] exists; values differ only in case". The guide says SPIT warns about paths that differ only in case, but here the paths belong to different products, so no warning fired.
- The first `dag` error reports only the first failing store. A hint such as "run `spit artifacts` for all" would help.
- `spit check` on the discover recipe said "Recipe valid." even though the recipe could never succeed on this data. That is expected, since check reads no data, but it is worth noting.

## 7. Language friction
There is no direct way to say "exclude stores X, Y, Z this week". I expressed the exclusion as data-quality rules instead, which is arguably better. `discover` combined with a stray file was unusable.

## 8. Compared with a shell script, Make, or Snakemake
It was faster for the diagnosis. `spit artifacts` gave the full cause tree in one command, where Make or Snakemake would fail on the first missing input. Producing the filtered plan was about as fast as a shell loop, once I found that `skip` without `discover` works.

## 9. Top three changes
1. When a source lookup fails, suggest near-miss values, especially ones that differ only in case (`S07` vs `s07`). Also warn about source artifacts that no job consumes.
2. Document how `per [...]` groups are formed, and whether a group with zero artifacts of the counted product is counted. Also document what to do about files outside the `discover` contexts, or allow skipping them with a warning.
3. Have `dag`'s first error point to `spit artifacts` for the full list of failures.
