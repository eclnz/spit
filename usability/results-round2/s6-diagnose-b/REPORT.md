# Report

## 1. Outcome

Yes. `ANSWER.md` identifies s03, s07, and s09. `plan.spitin` excludes them and `plan.spitdag` was written by `spit dag ... -o`. Confidence: **5/5**. The generated DAG has 22 jobs: nine clean, nine price, three store reports, and one chain summary over s01, s02, and s05. It has no `left_out` artifacts.

## 2. Walkthrough

1. Read the task, guide, pipeline, recipe, and file list; checked the recipe with `spit check`, which said `Recipe valid.`
2. Ran `spit artifacts` on the original recipe. It identified the s03 minimum of two weeks, the s07 case mismatch, and the missing s09 pricing artifact. It also showed the chain summary as incomplete.
3. Ran ordinary `spit dag` on the original recipe. It failed with "no `pricing` artifact for input `prices` of `price` at [store=s07,week=2026-W36]", followed by "pricing[store=S07] exists; its `store` differs only in letter case". I took this as confirmation that the pricing file's capitalization prevents the join.
4. Wrote `plan.spitin` with store group exclusions and built a first DAG. The planner reported "1 source artifact is used by no job (pricing: 1)". I realized the s07 group exclusion does not match uppercase `S07`, so I added an explicit exclusion for `pricing[store=S07]` and regenerated the DAG.
5. Checked the final DAG and `--commands` output. It has 12 external source artifacts, 22 jobs, and the chain command `chain_summary reports/s01.pdf reports/s02.pdf reports/s05.pdf --out reports/chain.pdf`.

## 3. Stuck points

The longest pause was deciding how to remove the stray uppercase pricing artifact while leaving the data file untouched. An explicit source exclusion resolved it.

## 4. Guesses

I expected `exclude [store=s07]` to leave `pricing[store=S07]` because entity values are case sensitive. The first DAG confirmed this. I also used `--root` with a recipe in the working folder so paths still resolve under `data/`; that worked.

## 5. Guide gaps

The guide explained `artifacts`, `exclude`, case-sensitive values, and `--root` well enough for this task. Its linked example files were unavailable, so I relied on the examples printed in the guide.

## 6. Error messages

The ordinary DAG failure's "pricing[store=S07] exists; its `store` differs only in letter case" hint was especially useful. The `spit artifacts` report's "input `weeks` of `store_report` needs at least 2 artifacts at [store=s03], found 1" was precise. No error message misled me.

## 7. Language friction

Excluding the logical s07 store required two rules because the misnamed uppercase pricing artifact had a different entity value. That is correct behavior, but easy to overlook.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was faster here because `artifacts` surfaced all three causes and traced the failures into the summary. A shell script would have needed custom validation and reporting. I did not run the nonexistent business commands.

## 9. Top three changes

1. When a group exclusion leaves a case-near source behind, print a targeted note about it.
2. Show a short summary by store in `artifacts` for pipelines with a natural grouping dimension.
3. Include a self-contained example of diagnosing failures and then excluding whole groups for a weekly plan.
