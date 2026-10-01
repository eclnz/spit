# Report

## 1. Outcome
Did you produce the deliverables? Confidence (1–5) that they are exactly right, and why.

Yes. I wrote `plan.spit`, `plan.spitin`, and `plan.spitdag`. Confidence: 5/5. `spit check` accepted the recipe, and `spit dag --commands` showed all 39 expected jobs: 16 trains, 16 evaluations, six summaries, and one leaderboard. I checked the source paths, output paths, flags, and aggregate argument orders in that preview. The programs are unavailable, so I did not execute the jobs.

## 2. Walkthrough
Your attempts in order. For every `spit` error or surprise: the message (quoted), what you thought it meant, and what you did next.

I read `TASK.md`, `GUIDE.md`, and this report template, then listed the files under `data/`. I used the guide's `@ each(model)` to cross each observed config and seed pair with the models, `@ vary(seed)` for each summary, and `@ vary(model, config)` for the single leaderboard. I wrote the pipeline and a minimal recipe, then ran `./bin/spit check plan.spitin`: "Recipe valid." I ran `./bin/spit dag plan.spitin --root data --commands`; it reported "39 jobs resolved." The preview matched my expected commands and ordering, so I wrote the DAG with `-o plan.spitdag`. There were no errors or unexpected messages from SPIT.

## 3. Stuck points
Where did you stall longest, and what finally got you past it?

Choosing the driver for the ragged sweep took the most thought. The guide's "Products and dimensions" and "Operations and commands" sections explained that the config and seed pairs should drive training, while `@ each(model)` adds the model dimension without inventing seed files.

## 4. Guesses
What did you do that the guide did not confirm? Did any guess turn out wrong?

I guessed the simplest two-file setup would be a pipeline and a recipe in the working folder with `--root data`. The guide confirmed that layout. No guess turned out wrong.

## 5. Guide gaps
What did you look for in GUIDE.md and not find? Name the section you checked.

I found the necessary rules in "Dimension order", "Operations and commands", and "Where files live". I did not find a critical gap for this task. A short inline example showing `@ each` and two-dimension `@ vary` together would have reduced page jumping, though the guide links to a ragged sweep walkthrough.

## 6. Error messages
Messages that misled you or didn't help, and any that were especially good. Quote them.

There were no error messages. "Recipe valid." and "39 jobs resolved." were clear. "14 source files verified." was useful confirmation that the scan found the intended inputs.

## 7. Language friction
Anything you wanted to express that the language made awkward or impossible, and the workaround you used.

The task was expressible directly. I had to put `dimensions [model, config, seed]` in the pipeline to state the aggregate order, and use `@ each(model)` on the model input even though the model is the first command argument. This took some reasoning but worked cleanly.

## 8. Compared with a shell script, Make, or Snakemake
Was SPIT faster, slower, or about the same to reach a correct result here? Why?

About the same as a concise Snakemake pipeline, and faster than manually generating a job list with a shell script. The `@ each` and `@ vary` rules handled the ragged seeds and ordered collections, while `--commands` made validation quick. Learning the dimension rules was the main cost.

## 9. Top three changes
The three changes to SPIT or its guide that would have helped you most.

1. Put a complete ragged sweep example inline in the guide where `@ each` and `@ vary` are explained.
2. Show a count by operation in the DAG summary, such as 16 train, 16 evaluate, six summarise, and one leaderboard.
3. Add an optional compact command preview grouped by operation, so a long sweep is easier to audit.
