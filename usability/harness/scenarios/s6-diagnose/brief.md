# Task: why are store reports missing?

You have inherited a weekly store-reporting pipeline: `data/pipeline.spit`, with its recipe `data/weekly.spitin`. The dataset is in `data/`. This week's planning run failed, and the team suspects that some stores' reports cannot be produced.

**Part 1.** Find every store whose report cannot be produced, and the root cause for each. Be specific about what is wrong and where. Write this to `ANSWER.md` in your working folder.

**Part 2.** The business has decided that stores with problems are left out of this week's run until they are fixed. Produce `plan.spitdag` in your working folder, planning everything for the remaining stores, including the chain summary over those stores. Do not rename, edit, or delete data files, and do not change what the commands do.
