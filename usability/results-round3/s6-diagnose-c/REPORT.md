# Report

## 1. Outcome

Yes. I wrote `ANSWER.md`, `data/weekly_ready.spitin`, and `plan.spitdag`. Confidence: **5/5**. The final plain `dag` resolved 22 jobs: nine clean, nine price, three store reports (s01, s02, s05), and a chain summary over those three. `--commands` showed the expected commands and output paths. I did not run the programs.

## 2. Walkthrough

1. Read `TASK.md`, `GUIDE.md`, `REPORT.md`, and the supplied pipeline and recipe. Listed dataset paths.
2. Ran `spit dag data/weekly.spitin --paths --commands`. It stopped at: "no `pricing` artifact for input `prices` of `price` at [store=s07,week=2026-W36]" and added "pricing[store=S07] exists; its `store` differs only in letter case". I understood this as a case-sensitive store join, then followed its suggestion to run `artifacts`.
3. `spit artifacts data/weekly.spitin` showed s03's report had "needs at least 2 artifacts at [store=s03], found 1" and all three s09 revenue artifacts lacked pricing. It also showed the failures flowing into `summary`. I recorded those root causes in `ANSWER.md`.
4. Wrote a new recipe excluding s03, s07, and s09, then `spit check` reported "Recipe valid." A `dag -o` resolved 22 jobs, but noted "1 source artifact is used by no job (pricing: 1)". I realized `[store=s07]` did not exclude the misnamed `pricing[store=S07]`, so I added an explicit exclusion for that artifact.
5. Ran `dag --commands` to inspect every generated command, including the chain summary with reports for s01, s02, and s05. Regenerated `plan.spitdag`; the final run resolved 22 jobs without an unused-source warning.

The note about three files matching no source rule was initially surprising. It did not stop planning. The extra recipe I created inside `data/` increased that count from two to three.

## 3. Stuck points

The longest pause was deciding how to exclude s07 completely: the sales identity is `s07` while the pricing source identity is `S07`. The unused-source note after the first plan revealed the leftover artifact. An explicit `exclude pricing[store=S07]` removed it.

## 4. Guesses

I assumed a plain `dag` after exclusions would produce a clean chain summary using only included stores; the `--commands` output confirmed it. I also assumed an exclusion of `[store=s07]` would leave `pricing[store=S07]`; the first plan's unused-source note confirmed that. No guess turned out wrong.

## 5. Guide gaps

I found what I needed in "Find incomplete artifacts", "Recipes", and "Exclude named artifacts". I did not find an explicit example of cleaning up a case-mismatched source after excluding the intended group; the unused-source note made the required extra exclusion clear.

## 6. Error messages

Especially good: "pricing[store=S07] exists; its `store` differs only in letter case" identified the exact join problem. The s03 minimum-count message was also direct. "3 files under `data` match no source rule" was less helpful without listing them inline, though the message gave a command to list them.

## 7. Language friction

The affected store had two spellings, so one group exclusion could not remove both the sales identity and misnamed pricing source. I used a group exclusion plus a source-specific exclusion. Otherwise the recipe expressed the decision cleanly.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was faster for diagnosis: `artifacts` identified all missing reports and their upstream causes in one run, and `dag --commands` showed the exact remaining work. Hand-written shell logic would have needed explicit scans, joins, and minimum-week checks. I did spend some time learning the exclusion behavior.

## 9. Top three changes

1. Have `dag` show all incomplete top-level reports, or a short summary by store, when it fails; today it shows the first error and sends the user to `artifacts`.
2. When an excluded group leaves an unused source whose identity differs only by case, suggest the matching source-specific exclusion.
3. Show a few unmatched file names in the scan note, especially when a recipe file inside the dataset is counted, while retaining the full `--unmatched` option.
