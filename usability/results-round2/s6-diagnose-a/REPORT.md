# Report

## 1. Outcome

Yes. I wrote `ANSWER.md`, `data/weekly-good.spitin`, and `plan.spitdag`. Confidence: **5/5**. The plan has nine `clean`, nine `price`, three `store_report`, and one `chain_summary` jobs; the summary consumes only s01, s02, and s05 reports.

## 2. Walkthrough

I read the guide, pipeline, recipe, and CLI help. `spit check data/weekly.spitin` reported `Recipe valid.` I ran `spit inputs`, `spit artifacts`, and `spit inputs --unmatched` on the original recipe. Artifacts showed s03's one-week minimum failure, missing pricing for s09, and the s07/S07 case mismatch. `--unmatched` listed only `pipeline.spit` and `weekly.spitin`, which are not dataset sources.

I created `data/weekly-good.spitin` to exclude the three bad stores. Its first DAG run succeeded but noted `1 source artifact is used by no job (pricing: 1)`: the uppercase `pricing[store=S07]` was still present because a group exclusion for `store=s07` matches case exactly. I then explicitly excluded `pricing[store=S07]` and regenerated the DAG. It reported 12 verified source files and 22 resolved jobs, with no unused source note. I inspected the generated JSON for job counts, report paths, and the chain summary command.

## 3. Stuck points

The longest pause was deciding how to exclude the misnamed uppercase pricing artifact. I added a source-specific exclusion after seeing the unused-source note.

## 4. Guesses

I expected `exclude [store=s07]` to leave `pricing[store=S07]` because identity values are case-sensitive; the first plan confirmed that. I used the guide's source-specific `exclude` syntax to remove it. No guess turned out wrong.

## 5. Guide gaps

I checked “Find incomplete artifacts” and “Exclude named artifacts.” They covered what I needed. A brief example showing a bad store excluded along with a case-mismatched orphan input would have shortened this task.

## 6. Error messages

No `spit` command errored. The useful diagnostic was: `pricing[store=S07] exists; its store differs only in letter case`. The first plan's `1 source artifact is used by no job (pricing: 1)` note prompted the final exclusion. The two unmatched files were pipeline and recipe files, so that note did not indicate bad data.

## 7. Language friction

Removing s07 required two exclusions because its pricing artifact carries a different store value. This is precise but easy to overlook.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was faster here. `artifacts` traced all incomplete reports and their causes in one invocation, and `dag` produced the plan after a short recipe edit. A shell script would need custom joins, cardinality checks, and dependency handling.

## 9. Top three changes

1. Show orphan inputs in the plan command with their full identities, so `pricing[store=S07]` is visible without another command.
2. Offer a recipe example for excluding a bad group and its differently named orphan source.
3. In `--unmatched` notes, distinguish pipeline and recipe files from likely missed data files.
