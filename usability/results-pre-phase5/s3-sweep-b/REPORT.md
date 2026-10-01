# Report

## 1. Outcome

Yes. I wrote `sweep.spit`, `sweep.spitin`, and `plan.spitdag`. Confidence: 5/5. `spit dag --commands` showed the expected 41 jobs: 16 train, 16 evaluate, 6 summarise, 2 per-model leaderboards, and 1 overall comparison. The rendered commands have the requested flags, paths, and collection order.

## 2. Walkthrough

I read `TASK.md`, then `GUIDE.md`, especially “Products and dimensions” and “Operations and commands.” I listed the data files and read “Ragged sweep: correlated seeds and collection order” in `docs/examples.md`. I used the seed artifacts as the training driver and `model @ each(model)` to cross each observed config/seed pair with every model. I declared the summary dimensions as `[model, config]`, then used successive `@ drop(seed)`, `@ drop(config)`, and `@ drop(model)` aggregations. I wrote a recipe to scan `data/` with `--root data`.

`bin/spit check sweep.spitin` returned “Recipe valid.” `bin/spit dag sweep.spitin --root data --commands` reported “41 jobs resolved.” I inspected the rendered commands, including warmup's two seeds, seed order in summaries, config order in both leaderboards, and model order in the final comparison. `bin/spit dag sweep.spitin --root data -o plan.spitdag` reported “wrote the .spitdag to `plan.spitdag`.” There were no errors or unexpected outputs.

## 3. Stuck points

I did not get stuck. The part that took the most thought was choosing the training driver so a config's seeds stay correlated, rather than making a Cartesian product of all configs and all seed values. The ragged sweep walkthrough settled that.

## 4. Guesses

I expected the derived training and metrics products to retain the broadcast model dimension, and the first DAG showed that. I also expected a `many` input to sort in the declared input product's dimension order; the guide states this, and `--commands` confirmed it. No guess turned out wrong.

## 5. Guide gaps

I did not find a needed feature missing from the guide. “Operations and commands” explains `each`, `vary`, `drop`, and collection ordering. The guide's ragged sweep example has only one final aggregation, so I had to apply the same rule twice more for the per-model and overall leaderboards, but that was straightforward.

## 6. Error messages

There were no errors or misleading messages. “Recipe valid.” was clear. The notes “14 source files verified” and “41 jobs resolved” were useful cross-checks.

## 7. Language friction

No blocking friction. Repeating explicit `path` and `command` lines for each stage is verbose, but it kept the exact requested command lines visible. I needed an explicit `[model, config]` summary dimension to make the later collection order clear.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was about as fast to reach this checked plan because the guide has a very close ragged sweep example. A short shell script might be quicker to draft, but I would need more work to verify all paths, missing seeds, and ordered aggregation inputs. The rendered DAG made those checks quick.

## 9. Top three changes

1. Add an example with two successive `many` aggregations, showing the resulting dimensions and argument order at each step.
2. Show a compact per-operation job count in `dag` output, so I can verify 16/16/6/2/1 without counting lines.
3. Put the `--root` recipe usage beside the first end-to-end example, since keeping pipeline files outside the dataset is common.
