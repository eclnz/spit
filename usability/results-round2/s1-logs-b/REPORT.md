# Report

## 1. Outcome
Did you produce the deliverables? Confidence (1–5) that they are exactly right, and why.

Yes. I wrote `weekly.spit`, `weekly.spitin`, and generated `plan.spitdag` with `spit dag ... -o`. Confidence: 5/5 for this dataset. SPIT resolved 20 digest jobs, three server rollups, and one fleet summary. `dag --commands` showed the requested argument order, paths, date order, and server-name order. No log dates or servers are listed in the pipeline.

## 2. Walkthrough
Your attempts in order. For every `spit` error or surprise: the message (quoted), what you thought it meant, and what you did next.

I read the guide, then wrote a source with `[server, date]`, a digest operation, a `many` rollup dropping `date`, and a `many` fleet summary dropping `server`. I put `weekly.spitin` beside the pipeline and used `--root` to point to `data`. `spit check weekly.spitin --path-rules` said `Recipe valid.` and showed all four explicit paths.

`spit inputs weekly.spitin --root data --unmatched` listed `logs/README.txt`, `logs/web1/2026-08-31.log.gz`, and `logs/web2/2026-09-07.log.1`. That was expected: none matches the complete `logs/{server}/{date}.log` source path. Its note said `found 20 source artifacts`.

`spit dag weekly.spitin --root data --commands` printed all 24 commands. The missing db1 day did not cause a gap; its rollup included the six existing dates. The three rollups ordered dates correctly and the fleet command ordered db1, web1, web2. Finally, `spit dag weekly.spitin --root data -o plan.spitdag` reported `24 jobs resolved` and wrote the file. There were no errors or unexpected SPIT messages.

## 3. Stuck points
Where did you stall longest, and what finally got you past it?

The main pause was deciding how to make the second aggregation produce one fleet artifact with no dimensions. The guide's Operations and commands section explains that one aggregate can drop several dimensions and that `many` inputs use `@ vary`; applying the same pattern twice solved it.

## 4. Guesses
What did you do that the guide did not confirm? Did any guess turn out wrong?

I expected `--root data` to make the recipe's path rules relative to `data`, and expected whole-path matching to reject `.log.gz` and `.log.1`. The Where files live and Paths sections confirm these, and the CLI output verified them. No guess turned out wrong.

## 5. Guide gaps
What did you look for in GUIDE.md and not find? Name the section you checked.

I looked in Resolve jobs and Operations and commands for a single complete example of a two-level aggregation ending in a dimensionless output. The rules are present, but there is no end-to-end example of that exact shape. Several example links in those sections are unavailable in this folder, as the task warned.

## 6. Error messages
Messages that misled you or didn't help, and any that were especially good. Quote them.

There were no errors. `Recipe valid.` and `24 jobs resolved.` were useful checkpoints. The unmatched-file listing was especially useful for confirming that the rotated and compressed files were ignored. The note `3 files under ... match no source rule` alone would have been less useful without `--unmatched`.

## 7. Language friction
Anything you wanted to express that the language made awkward or impossible, and the workaround you used.

No blocking friction. A source path template was enough to exclude the non-daily files, and `@ drop(date)` followed by `@ drop(server)` expressed the two levels of grouping. The command templates kept the different argument order required by `rollup` and `fleetsum`.

## 8. Compared with a shell script, Make, or Snakemake
Was SPIT faster, slower, or about the same to reach a correct result here? Why?

About the same as a short shell script for this one dataset, after reading the guide. SPIT took more setup syntax, but it supplied input discovery, dependency ordering, and a concrete command listing to check. For changing server and date inventories, the reusable pipeline would likely save work.

## 9. Top three changes
The three changes to SPIT or its guide that would have helped you most.

1. Include a complete two-level aggregation example that ends in a dimensionless fleet output.
2. Bundle the example files that GUIDE.md links to, or include the relevant example text inline.
3. Add a short recipe-plus-`--root` walkthrough for the common layout where the pipeline is beside a `data` directory.
