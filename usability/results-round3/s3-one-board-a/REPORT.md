# Report

## 1. Outcome

Yes. I wrote `sweep.spit`, `sweep.spitin`, and `plan.spitdag`. Confidence: **5/5**. `spit dag --commands` printed 39 jobs: 16 train, 16 evaluate, six summarise, and one leaderboard. I checked the command flags, paths, seed order, and leaderboard order against the task. The tools themselves do not exist, so this verifies the plan, not execution.

## 2. Walkthrough

I read `TASK.md` and `GUIDE.md`, then followed the guide's link to the ragged sweep walkthrough in `docs/examples.md`. I listed the files under `data/`, which showed two models, three configs, eight seed files, and one test set. I wrote a pipeline with `seed[config, seed]` as the driving input and `model @ each(model)` as the broadcast input. I declared `summary` explicitly as `[model, config]` so the final collection sorts in that order. I added a minimal recipe pointing to the pipeline and scanned `data/` with `--root data`.

`spit check sweep.spitin` returned `Recipe valid.` `spit inputs sweep.spitin --root data` reported “note: found 14 source artifacts under data” and listed the expected files. `spit dag sweep.spitin --root data --commands` reported “note: 39 jobs resolved.” Its commands matched the requested arguments and paths. Finally, `spit dag sweep.spitin --root data -o plan.spitdag` reported that it wrote `plan.spitdag`. I encountered no SPIT errors or unexpected results.

## 3. Stuck points

The longest pause was determining how to cross every model with the *observed* config and seed pairs while keeping the final collection model first. The ragged sweep walkthrough gave the exact `@ each(model)` pattern and explained why `summary : Summary [model, config]` matters.

## 4. Guesses

I inferred that scanning the recipe with `--root data` would make both input and output paths relative to `data/`; the printed commands confirmed it. The guide covered the remaining syntax I used. No guess turned out wrong.

## 5. Guide gaps

I found no missing rule for this task. The relevant sections were “Products and dimensions,” “Operations and commands,” “Paths,” and “Recipes.” The linked ragged sweep walkthrough was especially close to this task.

## 6. Error messages

There were no errors. `Recipe valid.`, the note about 14 source artifacts, and `note: 39 jobs resolved.` were clear. The `--commands` output was the most useful check because it showed the aggregate input order directly.

## 7. Language friction

The inferred dimensions after broadcasting are config first, so I had to restate `[model, config]` on `summary` solely to control the leaderboard argument order. This was a small extra step; the guide explained it.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was faster here because the ragged sweep walkthrough nearly matched the task and the planner showed every expanded command. For a shell script I would need to write and check the nesting, grouping, and ordering myself. I cannot compare actual execution since the named programs are absent.

## 9. Top three changes

1. Put a brief `each` plus ragged `vary` example in the main quick-start section, with a link to the full walkthrough.
2. Highlight that derived dimension order can differ from the desired `many` argument order, and show the explicit dimension declaration beside the broadcast example.
3. Add a compact `dag` summary by operation (for example, `train: 16, evaluate: 16`) to make counts easier to check without reading every job.
