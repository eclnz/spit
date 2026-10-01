# Report

## 1. Outcome

Yes. I wrote `pipeline.spit`, `dataset.spitin`, and generated `plan.spitdag` with `spit dag ... -o`. Confidence: **5/5**. The DAG has 41 jobs: 16 training, 16 evaluation, 6 summaries, 2 model leaderboards, and 1 comparison. I independently compared every resolved output path and command argument against the requested sweep, including the unequal seed sets and aggregate ordering. There are no left-out artifacts.

## 2. Walkthrough

1. Read `GUIDE.md`, focusing on “Products and dimensions,” “Operations and commands,” “Recipes,” and path rules. Listed the input files to see the actual model, config, and seed families.
2. Wrote a pipeline with sources for models, configs, seeds, and the test set; added paths and five operations. Wrote a recipe pointing to that pipeline. `spit check dataset.spitin --path-rules` said `Recipe valid.` and listed explicit path coverage for all nine products.
3. My first training call was `train_model(model, config @ each(config), seed @ each(seed))`. `spit dag ... --commands` failed: “no `seed` artifact for input `seed_file` of `train_model` at [config=warmup,model=large,seed=33]” and “8 more artifacts cannot be produced.” I understood that `each(seed)` broadcast the globally observed seed values, including 33, into `warmup`. I changed the call to `train_model(model @ each(model), config, seed)`, letting the existing `[config, seed]` seed file drive each run while models broadcast across it.
4. The next `spit dag ... --commands` resolved 41 jobs. I inspected the commands and their ordering, then generated `plan.spitdag` using `spit dag ... -o` with `--root` set to `data/`.
5. I parsed the DAG JSON and compared its full set of 41 operation names, output paths, and command arguments with an independently constructed expected set. The sets matched and `left_out` was empty.

## 3. Stuck points

The longest stall was expressing the uneven config-to-seed relationship. The first attempt formed a global config-by-seed cross product. Using the seed source as the driving artifact, then broadcasting only models, fixed it.

## 4. Guesses

I guessed that `seed @ each(seed)` would add only seed values belonging to the current config. That was wrong. I then inferred that the source with `[config, seed]` should drive the training operation; the resolved commands confirmed that inference. I also expected `many` inputs to sort in natural order as documented, which the resolved commands confirmed for seeds, configs, and models.

## 5. Guide gaps

In “Operations and commands,” the ragged sweep example shows `trial = simulate(reading, seed @ each(rep))`, but it does not show a complete multi-level sweep where one source already binds `[config, seed]` and another independent family of models must be crossed with it. A worked example identifying the driving input and explaining when `each` uses global values would have prevented my first failed DAG.

## 6. Error messages

The message “no `seed` artifact for input `seed_file` of `train_model` at [config=warmup,model=large,seed=33]” was useful: it named the exact invalid combination created by my call. “8 more artifacts cannot be produced” was less useful on its own, though the same message pointed to `spit artifacts` for details. `spit check` found no issue with the erroneous broadcast, which is understandable because the mismatch depended on the dataset.

## 7. Language friction

The driving-input rule is implicit in the call: the input with the most dimensions drives the operation. It took a failed resolution to learn that `each(seed)` did not respect config-specific membership. Writing `train_model(model @ each(model), config, seed)` is concise once that behavior is clear.

## 8. Compared with a shell script, Make, or Snakemake

For this small dataset, reaching the first correct plan was slower than writing nested shell loops because I had to learn SPIT's dimension and broadcast rules. The resulting pipeline is shorter than an explicit job list, validates the complete plan before running, and needs no edit for additional models, configs, or config-specific seed files.

## 9. Top three changes

1. Add a complete ragged model/config/seed sweep example to “Operations and commands,” with the seed source as driver and model broadcast.
2. Explain that `each(dimension)` draws values from its source across the inventory and may form combinations for which another input has no matching artifact.
3. Show a compact preview of output dimensions for each step in `spit check` or a dedicated CLI view, so it is easier to see which input drives a call before resolving data.
