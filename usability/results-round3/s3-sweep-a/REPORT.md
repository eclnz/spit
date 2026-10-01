# Report

## 1. Outcome

Yes. I wrote `sweep.spit`, `sweep.spitin`, and `plan.spitdag`. Confidence: **5/5**. The plan has 41 jobs: 16 train, 16 evaluate, 6 summarise, 2 leaderboard, and 1 compare_models. I inspected `dag --commands`: paths, flags, and the seed, config, and model collection orders match the task. I did not run the named training programs because they are unavailable and this is a planning task.

## 2. Walkthrough

I listed the files under `data` and saw two models, three configs, and eight config-specific seed files. I read the relevant sections of `GUIDE.md` and the ragged sweep in `docs/examples.md`. I wrote a pipeline where the seed file drives training and the model is broadcast with `@ each(model)`. I wrote a recipe under `data/`, checked the pipeline and recipe, and previewed the DAG commands.

Both checks succeeded with `Pipeline valid.` and `Recipe valid.`. The first DAG preview printed `note: 1 files under \`data\` match no source rule; \`spit inputs data/sweep.spitin --unmatched\` lists them`. I took that to mean the recipe file itself, which I had put inside the scan root. I moved the recipe to the working folder, used `--root data`, and the note disappeared. The preview resolved 41 jobs. I then ran `spit dag sweep.spitin --root data -o plan.spitdag` and inspected its job counts and root in the JSON.

## 3. Stuck points

The main reasoning point was making each config's observed seeds drive the jobs while crossing in every model. The guide's ragged sweep walkthrough gave the exact `seed_file` plus `model @ each(model)` pattern. I did not get stuck on a SPIT error.

## 4. Guesses

I initially guessed it would be convenient to put the recipe in `data/`. That worked, but the scan counted the recipe as an unmatched file, so I moved it. I also expected `@ vary(seed)`, then `@ vary(config)`, then `@ vary(model)` to preserve the requested argument order; `dag --commands` confirmed this.

## 5. Guide gaps

I checked “Where files live,” “Products and dimensions,” and “Operations and commands.” I found the syntax and ordering rules I needed. I did not find a direct reminder that placing a recipe inside a scanned dataset will make it appear in the unmatched-file count. That note was harmless but initially looked like a data mismatch.

## 6. Error messages

There were no error messages. `Pipeline valid.`, `Recipe valid.`, and `note: 41 jobs resolved.` were clear. The unmatched-file note quoted above was useful, but it did not identify the path; I had to infer it was the recipe or run the suggested `--unmatched` command.

## 7. Language friction

The pipeline required five separate operation declarations, commands, paths, and assignments for five straightforward stages. That is somewhat verbose, but the dimensions and `vary` clauses expressed the ragged sweep directly. No required relationship appeared impossible.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was about the same effort as a short Snakemake workflow for this task, and more setup than a quick shell script. Its command preview and validation made it faster to gain confidence that the uneven seed counts and ordered aggregate inputs were right. A shell script would require loops and explicit sorting to get the same stable argument order.

## 9. Top three changes

1. Add a small cookbook example with multiple nested aggregates: seeds to per-config summaries to per-model boards to one final board.
2. Have the unmatched-file note name the unmatched paths when there are only a few, especially a `.spitin` inside the scan root.
3. Provide a compact CLI summary of job counts per operation when writing a `.spitdag`, so users need not preview every command or inspect JSON just to verify cardinality.
