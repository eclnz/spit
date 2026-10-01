# Report

## 1. Outcome

Yes. I wrote `sweep.spit`, `sweep.spitin`, and `plan.spitdag`. Confidence: 5/5. `spit dag --commands` showed 41 jobs: 16 train, 16 evaluate, 6 summarise, 2 leaderboard, and 1 compare_models. I checked the emitted commands, paths, and collection order against the task.

## 2. Walkthrough

I read `TASK.md`, `GUIDE.md`, and the ragged sweep section of `docs/examples.md`, then listed the input filenames under `data/`. I wrote a pipeline using the observed `seed_file[config, seed]` pairs as the training driver and `model @ each(model)` to cross in each model. I used `@ drop(seed)`, `@ drop(config)`, and `@ drop(model)` for the three aggregation levels, with an explicit `[model, config]` summary declaration. I wrote a recipe to scan the dataset using the source path rules. `bin/spit check sweep.spitin` returned `Recipe valid.` The first `bin/spit dag sweep.spitin --root data --commands` resolved 41 jobs and showed the requested commands in the requested order. I then wrote `plan.spitdag` with `bin/spit dag sweep.spitin --root data -o plan.spitdag`.

There were no SPIT errors or unexpected output. The “found 14 source artifacts” and “41 jobs resolved” messages matched my count.

## 3. Stuck points

The main reasoning step was choosing the training driver so configs retain their own seed sets while models cross with every observed pair. The ragged sweep walkthrough resolved this quickly. I also had to ensure the summary product had `[model, config]` dimensions so later aggregates grouped and ordered correctly.

## 4. Guesses

I inferred that a bare `pipeline sweep.spit` recipe plus `--root data` would scan the `data/` folder using source path rules. The guide describes this, and the plan confirmed it. I made no unconfirmed syntax guesses that proved wrong.

## 5. Guide gaps

I found the needed syntax in GUIDE.md's “Operations and commands,” “Supply the inputs,” and “Where files live” sections. I did not find a major gap for this task. The full ragged sweep example in `docs/examples.md` was much quicker to adapt than piecing the pattern together from the language reference.

## 6. Error messages

There were no errors. `Recipe valid.` was clear. The “found 14 source artifacts” and “41 jobs resolved” messages made it easy to check coverage.

## 7. Language friction

The operation and product names are separate, so the five steps needed several declarations even though each operation is used once. This was manageable. Explicitly restating `[model, config]` for `summary` was necessary to control later aggregation order.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was probably faster than writing and checking a shell script for this ragged sweep. The `each` and `many` constructs made the correlated config/seed set and ordered aggregations concise, and `--commands` exposed the complete plan for inspection. Learning those constructs had some upfront cost.

## 9. Top three changes

1. Put a compact ragged sweep example directly in GUIDE.md near the `each` and `vary` descriptions.
2. Show a concise output dimension trace for each step during `check` or `dag`, especially after `each` and `drop`.
3. Add an optional `dag` summary count by operation so checking expected totals does not require counting a long command listing.
