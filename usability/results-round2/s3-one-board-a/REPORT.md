# Report

## 1. Outcome

Yes. I produced `pipeline.spit`, `dataset.spitin`, and `plan.spitdag` using `spit dag ... -o`. Confidence: 5/5. `spit check --strict-paths` passed, and the DAG contains 39 jobs: 16 train, 16 evaluate, 6 summarise, and 1 leaderboard. I inspected the rendered commands and the JSON for the final board. They match the requested paths, argument order, and collection order. No training or evaluation program exists here, so I did not run jobs.

## 2. Walkthrough

I read the guide, the CLI help, and the input filenames. I first made `model [model]` the driving source and broadcast `config` and `seed` with `each`. `spit check --strict-paths` said `Pipeline valid.` But `spit dag --commands` failed with: `no \`seed\` artifact for input \`seed\` of \`train\` at [config=warmup,model=large,seed=33]`, followed by `6 more artifacts cannot be produced`. I took this to mean `each(seed)` selected seed values across configs, then expected every config to have every selected seed. I changed the training call to drive from `seed [config, seed]`, joining the matching config and broadcasting only the model. I explicitly declared the output dimensions `[model, config, seed]`. The next check and DAG succeeded. The rendered commands showed exactly 16 training runs, matching the eight seed files crossed with two models. I generated the final `.spitdag` with `-o` and inspected its job count and final leaderboard input list.

## 3. Stuck points

The longest pause was choosing the driving source for a ragged sweep. The guide's `each` example looked close, but broadcasting the seed dimension after the config dimension produced missing seed combinations. Driving from seed files resolved it.

## 4. Guesses

I guessed that explicitly writing `[model, config, seed]` could reorder output dimensions even when the driving seed source declares `[config, seed]`. `spit check` and the resolved paths confirmed it. My first guess that separate `each(config)` and `each(seed)` would honor each config's own seed set was wrong.

## 5. Guide gaps

In “Operations and commands,” I looked for a direct explanation of how several `each` selectors interact when one source is grouped by another dimension. In “Products and dimensions,” I looked for an explicit statement that an output's declared dimension order may differ from the inferred order. The guide mentions explicit dimensions and has a ragged sweep example, but does not spell out these two behaviors together.

## 6. Error messages

The `no \`seed\` artifact ... at [config=warmup,model=large,seed=33]` message was useful: it exposed the unwanted combination precisely. The follow-up count, `6 more artifacts cannot be produced`, helped show the issue was systematic. I encountered no misleading error message.

## 7. Language friction

To get both the ragged seed set and the requested model-first summary and leaderboard order, I had to choose the seed file as the driver and then restate the output dimensions. This took experimentation. Otherwise, the path and command rules were straightforward.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was about the same time as a short shell loop for this one dataset. The first failed DAG and the guide lookup took time. SPIT gave a checkable artifact containing every job, dependency, and concrete command without writing a custom loop or manually enumerating seed files.

## 9. Top three changes

1. Add a complete example of a ragged sweep crossed with another independent dimension, including the driving source and output dimension order.
2. Explain how multiple `each` selectors choose values and why they can yield missing combinations.
3. State explicitly how declared output dimensions affect path binding and `many` input order, with a model and config aggregation example.
