# Report

## 1. Outcome

Yes. I left `sweep.spit`, `sweep.spitin`, the generated `sweep.spitout`, and `plan.spitdag` in the working folder. Confidence: **5/5**. SPIT resolved 41 jobs: 16 train, 16 evaluate, 6 summarise, 2 leaderboard, and 1 compare. I independently compared every DAG output path and command argument list against the files under `data/`; all matched, with no jobs left out. The named programs are unavailable, so I did not execute the planned jobs.

## 2. Walkthrough

I read `TASK.md`, `GUIDE.md`, the data file names, `REPORT.md`, and `spit help`. I looked up `each`, `vary`, ordering, source discovery, and path syntax in the guide. I wrote a pipeline with source dimensions `model`, `config`, and `[config, seed]`. The seed file drives training; `model @ each(model)` adds every model; the config source joins on `config`. Each later aggregation drops one dimension. I wrote a recipe pointing at the pipeline.

`spit check sweep.spitin --path-rules` reported `Recipe valid.` with all nine paths explicit. `spit inputs` reported `note: found 14 source artifacts under .../data`. `spit dag --commands` reported `note: 41 jobs resolved.` I inspected its printed commands, then generated `plan.spitdag` using `spit dag ... -o`. A separate JSON audit checked all 41 output paths and argument lists and confirmed `left_out` was empty. There were no SPIT errors or surprising messages.

## 3. Stuck points

The longest pause was deciding which input should drive training so configs could have different seed sets. The guide's `each` and `vary` explanation led me to use `[config, seed]` as the driving source and broadcast models. The first DAG confirmed the intended 16 training jobs.

## 4. Guesses

I inferred that the seed source should bind both `config` and `seed`, and that broadcasting `model` over it would leave `config` available for a normal join. The guide's linked ragged sweep example was absent, so I could not inspect it. The generated commands and independent audit confirmed this choice for the supplied files. I also inferred that future files following the same path patterns will be discovered without editing the pipeline; I did not add trial files to test that separately. No guess turned out wrong.

## 5. Guide gaps

In **Selectors** under **Operations and commands**, I wanted a complete inline ragged sweep example with the declarations, paths, and all three input roles. The guide refers to a ragged sweep example file, but that file is unavailable in this task. A short explanation of how the driving input is chosen when several inputs have dimensions would have made this quicker.

## 6. Error messages

There were no errors. `Recipe valid.`, `note: found 14 source artifacts`, and `note: 41 jobs resolved.` were useful confirmations. The `--commands` listing was especially useful because it exposed each ordered command before I wrote the DAG.

## 7. Language friction

I had to reason about the driving input and `each(model)` to express the ragged cross product. The syntax worked once I chose the seed source as the driver. The guide's aggregation syntax made seed, config, and model collection straightforward.

## 8. Compared with a shell script, Make, or Snakemake

For this first attempt, SPIT was slower than a short shell script because I had to learn its dimension and selector rules. It gave me a checked dependency graph, ordered aggregate arguments, and file discovery without writing loops, which should make later additions easier. I cannot compare actual execution because SPIT only planned jobs here.

## 9. Top three changes

1. Include the complete ragged sweep example inline in the guide, since linked examples may be unavailable.
2. Explain driving input selection and how `each` interacts with a joined input using a three input example.
3. Add a CLI example for auditing a DAG's job count, expanded commands, and ordered `many` inputs together.
