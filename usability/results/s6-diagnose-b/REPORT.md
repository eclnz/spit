# Report

## 1. Outcome
Produced ANSWER.md, plan.spitdag (22 jobs, stores s01/s02/s05 + chain summary) and data/plan.spitin (recipe). Confidence 4/5 that plan.spitdag is exactly right. The jobs and commands are what spit printed, and I checked the .spitdag has no s03/s07/s09/S07 jobs. The doubt: the `.spitdag` records `"root"` as an absolute path (/srv/spit-trials/s6-diagnose-b/data), and I am not sure whether a checker expects the recipe to sit elsewhere, or whether the extra `data/plan.spitin` file is acceptable.

## 2. Walkthrough
1. Read TASK.md, GUIDE.md, looked at data/. Spotted by eye: s03 has one sales file, pricing has `S07.json` (capital) and no s09 file. Wanted spit to confirm.
2. `spit check data/weekly.spitin` -> "Recipe valid." `spit inputs data/weekly.spitin` worked, no complaints about anything (note: "found 21 source artifacts under `data`"). Nothing flagged the odd `pricing[store=S07]`.
3. `spit dag data/weekly.spitin` -> `error: line 18, column 26: no 'pricing' artifact for input 'prices' of 'price' at [store=s07,week=2026-W36]`. Stops at the first failure, so only s07 shown, and "line 18" is a line in pipeline.spit though I gave the recipe (confusing: the file is not named).
4. `spit artifacts data/weekly.spitin` -> the complete list of problems (s07, s09 via price; s03 via min(2); summary). Very useful. It exits 0 even though things are missing.
5. Part 2: guide only shows `skip <discovery> count>=N per [dim]` examples. Tried skip rules in a throwaway recipe via `spit inputs`. `skip sales week=2026-W36,2026-W37 per [store]` worked (dropped s03 and S07). `skip sales store=s07 per [store]` -> `error: coverage rule for 'sales' requires values of 'store', which must be a dimension of that product outside its groups` (so you cannot skip a named store; I understood it: the value clause has to be a non-grouping dimension). Then found `skip pricing count=1 per [store]` drops s07 and s09 because they have 0 pricing in their group. Final recipe data/plan.spitin; `spit dag data/plan.spitin -o plan.spitdag` worked.

## 3. Stuck points
Longest: finding a way to say "leave out these stores" without editing data or the pipeline. The guide's skip section only talks about `sessions` discoveries; I had to experiment to learn that `skip <source product> ...` works without a `discover`, and that a group with zero artifacts of that product counts as count 0 (this is what made `skip pricing count=1` drop s09 and s07). That is behavior I inferred from the warnings, not from the guide.

## 4. Guesses
- That `skip` applies to a source product name (not just a discover rule name) without a discover. Guide says `require image count>=2 per [subject, visit]` for products, so plausible, and it worked.
- That a group with no artifacts of the skipped product is counted as 0 and therefore skipped. Worked, unconfirmed by the guide.
- That I may put a new recipe in data/ (TASK says adding files is ok). The recipe folder is the dataset root, so this keeps paths the same as the original.
- Wrong guess: `skip sales store=s07 per [store]` (errored).

## 5. Guide gaps
- Language reference > Recipes / Constraints: no statement of how count is computed for a group that has no artifacts of the product at all (0 vs not seen), nor of what `per [dim]` groups are built from (union of all products' values?). This decides whether skip can remove stores that lack a file.
- No way documented to exclude a specific named value (e.g. skip store=s07). Search in "Constraints"/"Discover contexts" found nothing.
- The guide does not say the `error: line N` of `dag` refers to the pipeline file when given a recipe.
- Nothing about case sensitivity of entity values, except the warning about artifacts whose paths differ in case; the S07 vs s07 mismatch has no hint anywhere.

## 6. Error messages
Good: the `artifacts` output ("input `weeks` of `store_report` needs at least 2 artifacts at [store=s03], found 1"; "no `pricing` artifact for input `prices` of `price` at [store=s07,...]"). Clear and traced through dependents.
Unhelpful: for s07, nothing suggests that `pricing[store=S07]` exists and probably is the intended file (a "did you mean / differs only in case" hint would have saved diagnosis). `spit inputs` silently accepts pricing for a store with no sales (S07).
`error: line 18, column 26:` on a recipe run gives no filename.
`spit dag` exiting at the first failure shows only s07, which might mislead someone into thinking it is the only bad store.

## 7. Language friction
Wanted "exclude these stores" / "only stores that are complete across sources". Workaround: two skip rules, one per source product, with count conditions chosen to reject the bad groups. It is indirect: `skip sales count>=2 per [store]` is really "stores with < 2 sales", and it is coincidentally the same as the pipeline's min(2). A cross-source "require every store that has sales to have pricing" is not expressible; I rely on the pricing group having count 0.

## 8. Compared with a shell script, Make, or Snakemake
Diagnosis was faster than a script for me: `spit artifacts` gave the full list of failures and reasons immediately. The plan part took longer than a shell `for` with an exclusion list would have, because of learning skip semantics by experiment. Overall about the same.

## 9. Top three changes
1. Document `skip`/`require` on source products (not only discoveries), and the treatment of groups with zero artifacts; add an example of excluding stores that lack a companion file.
2. Near-miss hint in the "no `X` artifact" error when another artifact of that product differs only in letter case (S07 vs s07); name the file in errors and in `dag`'s error line when a recipe is used.
3. Offer `spit dag` an option to report all incomplete artifacts (or point to `artifacts` in the error), and a simple way to exclude named values (e.g. `skip store=s07`) in recipes.
