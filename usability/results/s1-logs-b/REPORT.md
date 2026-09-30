# Report

## 1. Outcome
Yes. `plan.spitdag` (24 jobs: 20 logdigest, 3 rollup, 1 fleetsum), `logs.spit` and `logs.spitin` are in the working folder. Confidence 4/5. I checked the `.spitdag` by eye against the task: the 20 real daily logs (db1 has 6, missing 09-04; web1 7, web2 7) are found, and `web1/2026-08-31.log.gz`, `web2/2026-09-07.log.1` and `README.txt` are ignored. Digests are in date order, reports in server-name order, and the command lines and paths are as specified. The missing 1 point: I cannot run the real commands, and I am not certain that the order inside the `many` input is what the task means (see section 4).

## 2. Walkthrough
1. Read TASK.md, GUIDE.md, `spit help`. Did not open bin/spit.
2. Wrote `logs.spit` with one source `log [server, date]`, a `make_digest` op, a `roll` op (`many Digest`, `@ drop(date)`, `@ vary(date)`) and a `combine` op (`many Report`, `@ drop(server)`, `@ vary(server)`), with `path` rules per product. `spit check logs.spit --path-rules` said "Pipeline valid."
3. Wrote `logs.spitin` with `pipeline logs.spit` plus a `path log: ...` line (copying it, because I was unsure whether the recipe needed it). `spit check logs.spitin` and `spit inputs` then failed: "error: source `log` has path rules in both .spit and .spitin". I understood it at once, deleted the line from the recipe (recipe is now just `pipeline logs.spit`) and it worked.
4. `spit inputs logs.spitin --root /srv/spit-trials/s1-logs-b` printed 20 sources, the correct ones. Then `spit dag logs.spitin -o plan.spitdag`: 24 jobs resolved, no warnings.
5. Reviewed `dag --paths` and the JSON to confirm the commands, ordering and output paths.

## 3. Stuck points
No real stalls. The only mistake was the duplicated `path log:` rule in step 3 above. The longest thinking was about how to make the pipeline ignore `.log.gz` / `.log.1` / README without a filter rule; I relied on the path rule `{date}.log` matching only the whole file name, and the 20-artifact result confirmed it.

## 4. Guesses
- That a source path rule of `data/logs/{server}/{date}.log` would not match `2026-08-31.log.gz` or `2026-09-07.log.1`. The guide says files matching a rule are listed but never says the match is anchored at both ends. It turned out right.
- That `many` inputs are ordered by date / server name. The guide says "ordered by the product's dimensions with numbers compared as numbers". For the `digest` product the dimensions are [server, date], so within one server it is date order; for `week` it is server order. Confirmed in the output. It is a guess that ISO dates sort as strings in the right order; they did.
- That `rollup --out {output} {digests}` and `fleetsum {reports} -o {output}` in a command template are emitted with the words in template order (yes, they are).
- That `{output}` alone is enough for a single-output op without naming the port (guide says so; worked).
- That a product with no dimensions (`fleet`) can have a path without `{entities}` (worked; printed as `fleet[]`).
- The dataset root: `dag` from a recipe defaults to the recipe's folder. I put the recipe in the working folder so all paths are relative to it (`data/logs/...`, `digests/...`). The `.spitdag` has `"root"` as an absolute path (`/srv/spit-trials/s1-logs-b`), which I did not expect; the guide says paths in it are relative to the dataset folder, and does not mention an absolute root field.
- I used `--root` on `inputs` only; it was not needed for `dag`.

## 5. Guide gaps
- Section "Supply the inputs" / "Recipes": no statement that a source path rule may live in either the `.spit` or the `.spitin` but not both, and no advice on which to prefer. I only learnt it from the error.
- Section "Paths": nothing on whether path rule matching is anchored, nor how non-matching files (rotated, compressed, notes) are treated. A "dataset with extra files" example would have been reassuring.
- Nothing on whether a recipe with no `discover` rule is valid and complete (it is; `logs.spitin` is one line). I was unsure a recipe could be so minimal.
- Section "Operations and commands": no complete example of a command with flags on both sides of a `many` placeholder (like `-o {output}` after `{reports}`). It worked but was not confirmed.
- No mention of the `root` field in the `.spitdag`, or how a backend is supposed to use an absolute root.
- Section "How SPIT works" / "Write a pipeline": it is not said which of `.spit` path default vs. per-product `path product:` rules is preferred, other than `--strict-paths`.

## 6. Error messages
- Good: "error: source `log` has path rules in both .spit and .spitin" names the exact problem and both places. It did not give a line number, which would be a small improvement.
- Good: the notes from `dag` ("found 20 source artifacts", "20 source files verified", "24 jobs resolved") made it easy to check the counts against the dataset.
- No misleading messages met.

## 7. Language friction
Little. The naming was slightly awkward: a `many` op needs both `@ drop(date)` on the operation and `@ vary(date)` at the call, which repeats the dimension; I wrote both each time. The two type names `Digest` and `Report`/`Fleet` are needed only for the signature; I would have liked to omit them but the `->` syntax seems to require a type or a name (I did not try omitting). The operation names `roll` and `combine` differ from the program names, which is fine but forced me to keep two kinds of names in my head.

## 8. Compared with a shell script, Make, or Snakemake
About the same time to write, slightly slower for a first-timer, but faster to verify: the dag printout and the source count check made the result checkable without running anything. A shell script would have needed explicit handling of db1's missing day and of the `.gz` / `.1` files; here both were handled by the path rule. Make would need wildcard/`$(wildcard)` tricks and sorted lists for the rollups; Snakemake's `expand` with a directory listing is comparable. SPIT's advantage is the validated plan and no server/date listing in the pipeline.

## 9. Top three changes
1. Document the rule that a source path may be set in the pipeline or the recipe but not both, and state that a one-line recipe (`pipeline x.spit`) is enough; show this in the guide's "Recipes" section.
2. State that path rule matching is over the whole file name and that non-matching files (rotated/compressed/notes) are silently ignored; maybe have `spit inputs` optionally list files under the scan root that matched no rule, so a user can see what was ignored.
3. Explain the `.spitdag` `root` field (absolute path) and how a backend should treat it, and show one complete `many` command example with the output flag after the collected inputs.
