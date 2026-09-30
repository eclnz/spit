# Report

## 1. Outcome
Produced `plan.spitdag` (41 jobs: 16 train, 16 evaluate, 6 summarise, 2 leaderboard, 1 compare_models), plus `sweep.spit`, `sweep.spitin` (one line, `pipeline sweep.spit`) and `sweep.spitout` (written by `spit inputs`). I inspected the JSON: command lines, output paths, seed order (11,22,33) and config order (lr-high, lr-low, warmup) all match TASK.md; warmup has only 2 seeds and is handled without special cases. Confidence 4/5. I checked commands and paths by reading the JSON, not with an independent oracle. Two things I did not verify are the order in which `train` jobs are emitted (seed-major, then model), which TASK.md does not constrain, and whether a `.spitdag` consumer cares that the output entities are listed as config,seed,model.

## 2. Walkthrough
1. Read TASK, GUIDE, `spit help`. Wrote `sweep.spit` in one go: sources model[model], config[config], seedfile[config,seed], testset; `train(seedfile, model @ each(model), config)`; chained operations with `vary`/`drop`; explicit `path product:` rules for every product.
2. `spit check sweep.spit` -> `error: line 4, column 1: expected product name followed by [dimensions]`. This was for `source testset : TestSet` (a dimensionless source). I guessed it wanted `[]`, and `source testset : TestSet []` worked. The guide never shows a source with no dimensions (only `global` appears, as a path `{entities}` value).
3. Next error: `type mismatch at `train.model`: product `seedfile` is SeedFile, expected Model`. I had written the call in the order of the driving input, but the guide says call inputs follow declared port order, so I reordered the call to `train(model @ each(model), config, seedfile)`. Fine, but I had half-expected "driver position doesn't matter" to mean the call order was free.
4. `spit check` and `spit check sweep.spitin` passed. `spit inputs sweep.spitin -o sweep.spitout` found 14 sources, `spit dag sweep.spitin --paths` gave 41 jobs (I computed 16+16+6+2+1=41 beforehand), then `spit dag sweep.spitin -o plan.spitdag`. I checked the JSON with a small python script.

## 3. Stuck points
No long stalls. The longest pause was working out how to model "every model x every config x that config's seeds". My route was to use `each(model)` to broadcast the model source across the (config,seed) driver, which I found in the guide's `each` section. I had to convince myself from the text alone that it would be correct. I also wondered whether `config` would be a proper input or just come through `seedfile`. I made config a separate source because the command needs the config yaml path.

## 4. Guesses
- `source testset : TestSet []` for a dimensionless source (right, but unconfirmed in the guide).
- That an input with fewer dimensions than the driver (testset with none, config with one) needs no selector. The guide says inputs may use only dimensions the driver has, and this worked.
- That the recipe can be a single `pipeline` line, with the pipeline's own `path` source rules doing discovery when `spit inputs` scans the recipe's folder. The guide says path rules find sources, but does not say a recipe with no `discover` is valid. It worked.
- That scanning from the working folder (not `--root data`) is right, since the source paths are written as `data/...` and output paths are relative to the working folder.
- That `{seed}` and `{config}` placeholders in an output path rule are available for outputs whose dimensions came through `each` (`model`). Worked.
- That config-name ordering in `many` expansion is lexicographic. The guide only states "ordered by the product's dimensions with numbers compared as numbers". The result confirmed it, but I would not have known in advance what order `lr-high`/`lr-low`/`warmup` would get.

## 5. Guide gaps
- Dimensionless sources: no syntax shown (checked "Products and dimensions"). `[]` was needed.
- A recipe that contains only a `pipeline` line, without `discover`: not stated to be legal (checked "Recipes").
- Dimension order in derived outputs: the `weights` outputs came out as `[config,seed,model]` (driver dims then broadcast dim). The guide says `{entities}` uses "declared order" but does not say what the order is for a product gaining a dim via `each`. It does not matter here because I used explicit placeholders, but it would matter with `{entities}`.
- Whether the many-input sort uses dimension order of the product (config,seed) versus the group key: in `summarise`, grouping is per (model,config) and seeds sort numerically, which is what I needed, but I had to read the output to be sure.
- No guidance on where to put a recipe when data lives in a sub-folder and outputs go beside it (recipe folder = dataset root).
- Nothing said about whether a seed that is a number in a filename (`seed-11.json`) becomes the string `11`; the `.spitdag` stores it as a string `"11"`. Numeric sort still worked.

## 6. Error messages
- `expected product name followed by [dimensions]` was accurate about the grammar, and pointed at line 4 column 1, but did not tell me that `[]` is allowed or what to do for a dimensionless source.
- `type mismatch at `train.model`: product `seedfile` is SeedFile, expected Model` was good: it named the port and the product, which made the call-order cause obvious. It did not hint "call arguments follow port order".
- The duplicated message (`error: line 13 ...` then `error: in `sweep.spit` line 13 ...`) when checking a recipe or file is redundant noise.

## 7. Language friction
- Having to write `each(model)` on the `model` input and listing it first in the call, while the real "driver" is the last argument. The mental model (driver = most dimensions, wherever it sits) is fine but the call order is in tension with it.
- `source testset : TestSet []` looks odd.
- I needed a separate `config` source plus `seedfile` with dim `config` to bring config-specific seeds. It is natural, but there is no way to say "seedfile's config is the config" other than sharing the dim name, which I relied on implicitly.

## 8. Compared with a shell script, Make, or Snakemake
About the same or slightly slower for this first pipeline, given the learning cost. The irregular seeds (warmup has 2) fall out with no special case, which is the thing that would have bitten a shell loop, and the guarantee that adding a model, config, or seed needs no change is real. Validation was quick and I could count the expected jobs (41) and see that the tool agreed. Snakemake would have needed `expand` with a per-config seed lookup, so SPIT is at least as good there after the learning curve.

## 9. Top three changes
1. Document dimensionless (global) sources with an explicit example, and accept a bare `source testset : TestSet` if possible.
2. Add a worked example in the guide of the cross-product-with-ragged-inner-dimension pattern (model x config x seeds that differ per config) using `each`, and state that call arguments must follow port order even when the driver is elsewhere.
3. State in the Recipes section that a recipe may be just a `pipeline` line when path rules find the sources, and document the dimension ordering of outputs that gain dims through `each`, plus the sort order of string dimension values in `many` expansion.
