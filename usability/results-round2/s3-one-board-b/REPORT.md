# Report

## 1. Outcome

Yes. `plan.spit`, `plan.spitin`, and `plan.spitdag` are in the working folder. Confidence: **5/5**. SPIT resolved 39 jobs: 16 training, 16 evaluation, 6 summaries, and 1 leaderboard. I checked every saved command against the expected paths and argument order, including the seed order and model-then-config leaderboard order. The DAG has no left-out artifacts.

## 2. Walkthrough

I read the guide, CLI help, report template, and dataset file list. I wrote the pipeline with `model`, `config`, `seed`, and `testset` sources. Training uses `seed @ each(config, seed)` to add both config and seed to the model dimension. I checked the pipeline with `spit check ... --path-rules`; it reported `Pipeline valid.` I ran `spit dag ... --commands` and reviewed the expanded commands. It reported `39 jobs resolved.` I then moved my recipe from `data/` to the working folder, ran `spit dag ... --root data -o plan.spitdag`, and checked the saved DAG's commands with a small script.

The only surprise was: `note: 1 files under .../data match no source rule`. That file was my own `plan.spitin`, initially placed under `data/`. I moved it out of the dataset folder and supplied `--root` explicitly. The note disappeared. There were no SPIT errors.

## 3. Stuck points

The longest pause was choosing the driver and dimension order. The summary list must sort by model and then config, so I made `model` the training driver and broadcast the config and seed dimensions from the seed files. The expanded commands confirmed the order.

## 4. Guesses

The guide showed `@ each` for one dimension but did not explicitly show `@ each(config, seed)` on one source. I guessed that it accepts two dimensions and preserves the model-first output order. `spit check` and the resolved DAG confirmed this. I also expected the config input to join on the broadcast config dimension; the resolved commands confirmed that.

## 5. Guide gaps

In the "Selectors" part of the language reference, I looked for an explicit example of broadcasting multiple dimensions from one input. The ragged sweep note points to an example file that is unavailable here, so it did not answer this directly. A short inline example would help.

## 6. Error messages

There were no errors. `note: 1 files under .../data match no source rule` was useful: it identified the extra recipe file I had put in the dataset. `note: 39 jobs resolved.` was a useful count, though I still inspected the commands to confirm which jobs were included.

## 7. Language friction

Writing the full cross product through `@ each(config, seed)` took some reasoning because the seed list belongs to each config. The language expressed it compactly once I understood the broadcast rule. The separate `.spitin` file and explicit `--root` were a little setup for a simple scan.

## 8. Compared with a shell script, Make, or Snakemake

For this one task, SPIT was about the same speed as a concise Snakemake file and slower than a quick shell loop, mostly because I had to learn the dimension rules. It was easier to inspect the fully expanded job graph and command order than it would be with a shell loop.

## 9. Top three changes

1. Put a complete, inline ragged sweep example in the guide, including a multi-dimension `@ each` and the resulting paths.
2. Show the derived dimension order in `spit check --path-rules` or another inspection command, so aggregate argument order is clear before building a DAG.
3. Give a compact per-operation job count in `spit dag` output, such as `train: 16; evaluate: 16; summarise: 6; leaderboard: 1`.
