# Report

## 1. Outcome

Yes. I wrote `survey.spit`, `survey.spitin`, and `plan.spitdag` with `spit dag ... -o`. Confidence: **5/5**. The generated DAG contains 11 clean jobs, 4 model jobs, 4 charts, and 1 national table. I inspected its commands, paths, stages, dependencies, and fit verification records. The backup file is absent.

## 2. Walkthrough

I read the task, guide, questionnaire, CLI help, and dataset file list. The guide's `many` ordering rule said digit runs compare numerically, so the wave dimension could handle wave 10. I wrote a pipeline with the three requested stages, one multi-output fit operation, and a `verify fit` command. I used a one-line recipe to scan the dataset. `spit check survey.spitin --path-rules` reported `Recipe valid.` I then ran `spit dag survey.spitin -o plan.spitdag`, which reported `20 jobs resolved: 11 in ingest, 4 in model, 5 in publish.` Finally, I inspected the DAG JSON.

There were no SPIT errors. The DAG command noted `7 files under ... match no source rule`; I took that to include unrelated project files and the backup file, and confirmed that the resulting inputs contain only the 11 intended response CSVs. It also said `11 source files verified.` This is a source path and existence check, not CSV validation.

## 3. Stuck points

The most time went into confirming that one `many` input would order waves numerically and that the same expansion would be used by `verify` and `command`. The guide's Operations and commands section answered the ordering question; inspecting the DAG showed the exact argument lists.

## 4. Guesses

I assumed a fixed `path national:` was valid for a product with no dimensions, based on the guide's fixed-path source example. `spit check` accepted it. I also assumed `verify fit: validate_panel {waves}` would be recorded ahead of the fit and would prevent the fit if it fails; the guide explicitly says this, and the DAG contains the verify entry. No guess turned out wrong.

## 5. Guide gaps

I did not find a gap that blocked this task. I checked Operations and commands for natural ordering and verification, Stages for phase grouping, and Path rules for fixed and dimensional paths. The guide links to a model fit example that is unavailable here, but its inline multi-output and verify examples were sufficient.

## 6. Error messages

There were no error messages. `Recipe valid.` and `20 jobs resolved: 11 in ingest, 4 in model, 5 in publish.` were useful confirmations. The `7 files ... match no source rule` note is broad in this folder and initially needs interpretation; it did not indicate a missing job.

## 7. Language friction

The pipeline was compact. Expressing a two-output aggregate required coordinating `@ drop(wave)`, `@ vary(wave)`, the two named output ports, and their paths across several lines. That was workable, but it took care to check all four pieces together.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was about the same time for this small dataset, perhaps a little slower up front because I had to learn its syntax. Once written, the recipe automatically found the irregular waves and the DAG exposed numeric ordering, dependencies, and the validation gate for inspection. A hand-written shell script would need those rules implemented and checked manually.

## 9. Top three changes

1. Include a complete inline example combining `many`, multi-output, `verify`, and stages; the linked example is absent in this setup.
2. Make the unmatched-files note distinguish files under a source's data directory from unrelated project files, or show a short sample of paths.
3. Show a compact summary of the generated commands and verify gates when writing a DAG, so users can spot argument-order mistakes without opening the JSON or running a separate display command.
