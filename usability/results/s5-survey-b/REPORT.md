# Report

## 1. Outcome
Deliverables produced: `plan.spitdag` (written with `spit dag survey.spitin -o plan.spitdag`), `survey.spit` (pipeline) and `survey.spitin` (one-line recipe). Confidence 4 out of 5 that the plan is exactly right. The 20 jobs (11 clean, 4 fit, 4 plot, 1 national table), command lines, output paths, wave order (nw: 1, 2, 10) and region order were all checked in the JSON. The `.bak` file was not picked up. I am not fully sure that the `verify` mechanism is what the task means by "validate_panel must run before each fit and block it". Also, each job carries a `stage` field which the task did not list.

## 2. Walkthrough
1. Read TASK.md, GUIDE.md, REPORT.md, then `spit help`. I did not use `spit check --help` or other help pages.
2. Wrote `survey.spit` in one go: one `source response [region, wave]` with a path rule; stages ingest/model/publish; `fit_panel` as a two-output operation with `many` input `@ vary(wave)` and `@ drop(wave)`; a `verify fit_panel: validate_panel {waves}` line for the check; `plot_region`; `national_table` with `coefs @ vary(region)`; explicit `path` rules for every product.
3. Wrote `survey.spitin` containing only `pipeline survey.spit`.
4. `spit check survey.spit` printed "Pipeline valid." with no warnings. `spit inputs survey.spitin` found 11 sources, correctly ignoring `wave3.csv.bak`.
5. `spit dag survey.spitin --paths` resolved 20 jobs on the first try. I then wrote `-o plan.spitdag` and inspected the JSON with python to confirm commands, `verify`, and ordering.
No errors from spit at any point. Nothing surprised me in the output apart from what is listed below.

## 3. Stuck points
No real stall. The longest thinking was about how to express "validate_panel must run first and block the fit". The guide's `verify` section was the only candidate and it fits. I was uncertain whether the `.spitdag` would encode the verify as a gate on the fit (see below).

## 4. Guesses
- That `verify` is the right mechanism for validate_panel. The guide says a verify command "checks a job's inputs before its command runs ... if it fails, the job does not run", which matches the task, so it is a fairly safe guess. The JSON has it as `"verify": [[["validate_panel"], {path...}, ...]]` inside the fit job. Correct to the best of my reading, but I cannot confirm how a backend would treat it.
- That `wave{wave}.csv` (literal text touching a placeholder in a path rule) is allowed. It worked.
- That the recipe can be just `pipeline survey.spit`, with no `discover`, and the scan by path rule finds sources. The guide says path rules find sources (Paths section, last paragraph) so this was supported, but a minimal recipe is not shown as an example.
- That the dataset root is the working folder (recipe's folder), so `data/responses/...` and `build/...` paths are relative to it. Confirmed by `root` in the JSON.
- That the `many` placeholder `{waves}` expands in numeric wave order (10 after 2). Guide says so, and the JSON confirms it.
- That the `coefs` product (plural) name is fine for a multi-output assignment. It is only a naming choice.
None turned out wrong.

## 5. Guide gaps
- Nothing states how a backend treats `verify` failure with respect to the job and its dependents, or whether `verify` commands appear in the `.spitdag` (they do, under `verify`). Checked "Operations and commands".
- No example of a product with zero dimensions and a path rule (my `national`). It worked, but the Paths section only says `{entities}` becomes `global` for such a product.
- The guide says `inputs` scans "the recipe's folder" and the note said "found 11 source artifacts under `.`". It does not say whether scanning `build/` output (the same tree) could confuse things; it did not, because output paths differ from source paths.
- No advice on a recipe that has no `discover` rules at all (minimal recipe).
- The guide does not say which spit-managed fields (`stage`, `dependents`, `fingerprint`) show up in the `.spitdag` beyond the brief table description, so I cannot tell whether extra fields matter for "exactly those jobs".

## 6. Error messages
I got none, so I cannot rate them. The notes printed by `dag` were useful ("11 source files verified", "20 jobs resolved: 11 in ingest, 4 in model, 5 in publish").

## 7. Language friction
- Having `clean` be a product whose path rule lives in the pipeline and uses `wave{wave}` worked fine.
- Stages are mandatory-feeling for "phases" but a stage only groups products; the actual `build/ingest|model|publish` directories had to be written by hand in every path rule, since `{stage}` would give `ingest/clean/...` and I needed `build/ingest/clean/...`. Something like `path: build/{stage}/...` would not have matched the required layout (`coef`/`diag`/`chart` subfolders differ from product names), so explicit rules were correct anyway.
- Multi-output operation forced me to give the two outputs different product names (`coefs`, `diags`) and separate path rules, which is fine.

## 8. Compared with a shell script, Make, or Snakemake
About the same to a bit faster for a first correct result here, because irregular waves (missing, non-contiguous, wave 10 ordering) needed no special handling, and the `validate_panel` gate and multi-output fit are first-class. Snakemake would need a wildcard-glob function for the wave lists and explicit natural sorting; Make would be worse. Cost: reading the guide first, and no way to execute the result to verify it.

## 9. Top three changes
1. Document `verify` semantics fully (failure handling, dependents, how it appears in the `.spitdag`) and show a verify-before-expensive-step example, since this is a common pattern.
2. Show a minimal recipe (just a `pipeline` line) and a zero-dimension output product in the guide.
3. Say explicitly which fields a `.spitdag` job carries, and offer a compact human-readable `dag` view of the exact command line (`dag --paths` shows files but not the expanded command or verify line).
