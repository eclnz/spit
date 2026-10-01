# Report

## 1. Outcome

Yes. I wrote `pipeline.spit`, `inputs.spitin`, and `plan.spitdag`. Confidence: 5/5 for the supplied dataset. The DAG has 20 digest jobs, 3 server rollups, and 1 fleet job; I checked the expanded commands and ignored-file list.

## 2. Walkthrough

I read the guide's three-step workflow, operation/command rules, path rules, and recipe section. I inspected the dataset: three servers and 20 daily `.log` files, plus a notes file, a `.log.gz`, and a `.log.1`. I wrote a pipeline with source dimensions `[server, date]`, then used `@ drop(date)` for server rollups and `@ drop(server)` for the fleet rollup. I wrote a recipe pointing to that pipeline. `spit check ... --path-rules` said `Pipeline valid.` and showed all four explicit path rules.

My first DAG attempt combined `--commands` and `-o`; it failed with `error: --commands cannot be used with -o`. I took that to mean the command preview and file-writing modes are exclusive. I ran `dag --commands` for review, then a separate `dag -o plan.spitdag`. The preview showed exactly the requested command shapes and order. The file-writing run reported `24 jobs resolved.` and `wrote the .spitdag`. I used `inputs --unmatched` to confirm the three non-daily files were ignored.

## 3. Stuck points

There was no prolonged stall. The small interruption was discovering that `--commands` and `-o` cannot be combined; running them separately resolved it.

## 4. Guesses

I assumed `logs/{server}/{date}.log` is sufficient to recognize daily logs in this dataset, because all matching `.log` names are ISO dates. The guide does not describe a way to constrain `{date}` to a `YYYY-MM-DD` pattern. I made no failed language guess.

## 5. Guide gaps

In “Paths” and “Recipes,” I did not find a way to validate the format of a value captured by a path placeholder. That would matter if a future unrelated `.log` file were placed in a server directory. The CLI table did not explicitly say `--commands` and `-o` are mutually exclusive.

## 6. Error messages

`error: --commands cannot be used with -o` was clear and immediately actionable. `note: 3 files under ... match no source rule` was useful, and `--unmatched` listed precisely the notes, compressed, and rotated files. No message misled me.

## 7. Language friction

The `many` and `@ drop` clauses express both rollups directly. The only limitation I noticed was the lack of a date-format constraint on discovered path values. I relied on the supplied files' names and the `.log` suffix.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was about as fast as a short shell script for this small dataset. Its advantage was showing the complete 24-job plan and command ordering before any program ran. Learning the `many`, `vary`, and `drop` syntax took some reading.

## 9. Top three changes

1. Add an optional pattern or typed constraint for values captured by source paths, such as an ISO date.
2. Document the `--commands`/`-o` incompatibility in the CLI option table.
3. Add a complete in-guide example of two successive aggregations, ending in a global output.
