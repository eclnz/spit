# Report

## 1. Outcome
Did you produce the deliverables? Confidence (1–5) that they are exactly right, and why.

Yes. I wrote `fmri.spit`, `cohort.spitin`, `cohort.spitout`, and `plan.spitdag`. Confidence: 5/5. SPIT resolved 41 jobs (13 motion correction, 6 extraction, 13 coregistration, 6 session averages, 3 longitudinal combinations). I inspected the DAG programmatically and checked every command argument, output path, aggregate order, and the exclusion of sub-03. The named programs were not run, as requested.

## 2. Walkthrough
Your attempts in order. For every `spit` error or surprise: the message (quoted), what you thought it meant, and what you did next.

I read the guide, listed the dataset files, and used `spit help`. I wrote one pipeline with source and derivative path rules, five operations, and their commands. I wrote a recipe that discovers session directories and uses `drop [sub] where sessions count<2`. `spit check cohort.spitin --path-rules` returned `Recipe valid.` and showed explicit paths for all seven products. `spit inputs ... --root .../data -o cohort.spitout` reported `note: dropped [sub=03] by \`drop [sub] where sessions count<2\` (line 4); found 1` and `note: found 19 source artifacts and 6 contexts`. `spit dag ... --root .../data -o plan.spitdag` reported `note: 41 jobs resolved.` I then checked the DAG JSON against the requested commands. There were no SPIT errors or unexpected jobs.

SPIT also reported `note: 24 files under \`.../data\` match no source rule`. I understood these to be sidecars and top-level files, plus possibly other noninput files, so I did not change the recipe; the 19 selected source artifacts exactly matched the retained 13 BOLD runs and 6 T1w images.

## 3. Stuck points
Where did you stall longest, and what finally got you past it?

The main thinking step was choosing where to put the cohort rule and how to anchor paths. The guide's “Supply the inputs,” “Where files live,” and “Discover contexts from directories” sections showed that the `drop` belongs in `.spitin` and that `--root` should point to `data/`. I did not get stuck after that.

## 4. Guesses
What did you do that the guide did not confirm? Did any guess turn out wrong?

I inferred that the 24 unmatched files were intentionally irrelevant from the task and the selected source count. I did not run `--unmatched` to enumerate them. I made no syntax guesses that proved wrong.

## 5. Guide gaps
What did you look for in GUIDE.md and not find? Name the section you checked.

The “Resolve jobs” section links to a `.spitdag` reference file that was not provided in this trial. I inspected the generated JSON to verify how command argument paths were represented. Otherwise the guide covered this pipeline's needed features.

## 6. Error messages
Messages that misled you or didn't help, and any that were especially good. Quote them.

No errors occurred. The drop note quoted above was especially useful because it identified the excluded subject and the observed session count. The unmatched-files note was less informative without a breakdown by file type, but it provided the `--unmatched` follow-up command.

## 7. Language friction
Anything you wanted to express that the language made awkward or impossible, and the workaround you used.

The BIDS source names repeat `sub` and `ses` in both directory and filename, so each source path rule is long. The same is true of derivative paths. Explicit path rules handled it, with no workaround or unsupported behavior.

## 8. Compared with a shell script, Make, or Snakemake
Was SPIT faster, slower, or about the same to reach a correct result here? Why?

For this one small fixed dataset, SPIT took a little longer than a short script because I had to learn its recipe and aggregation syntax. It made the cohort exclusion, dependencies, and exact job list inspectable without writing custom enumeration code, so I expect it to pay off as the dataset changes.

## 9. Top three changes
The three changes to SPIT or its guide that would have helped you most.

1. Include the linked `.spitdag` reference in the trial guide or summarize its command-array format there.
2. Add a complete, self-contained example combining directory discovery, subject-level `drop`, a one-to-many join, and two nested aggregations.
3. Break down the unmatched-file note by suffix or category, so intentional sidecars are easier to distinguish from accidentally missed inputs.
