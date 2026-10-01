# Usability study: fourth round after the paths work

This round tested the [dataset root, output extensions and sidecars](design/output-paths.md) work: `ext:` and extensions on operation outputs, `@` placeholders, and optional path groups. It was kept small to limit cost. Three fresh participants on a smaller model each did one scenario: s1-logs (path rules), s2-cohort (BIDS paths and sidecars), and s5-survey (two outputs and `verify`). Each was told to read the guide once and keep its report short. There was no follow-up and no second participant per task. The [run archives](results-round4/) hold each pipeline, recipe, plan and report.

## Method and limits

The method was the same as [round 3](ROUND3.md): sandboxes from `harness/make_run.sh` and the logging `bin/spit` wrapper, graded with `harness/grade.py`. Each participant used about 75k tokens and between 7 and 8 tool calls. With one participant per task and a single model, this round can find friction but cannot compare rates with earlier rounds. Participant transcripts were not audited for external access.

## Results

All **3 plans matched their keys**.

| Task | Jobs | `spit` calls | Failed calls | Confidence |
| --- | ---: | ---: | ---: | ---: |
| s1-logs | 24 | 4 | 0 | 5 |
| s2-cohort | 41 | 6 | 2 | 4 |
| s5-survey | 20 | 3 | 0 | 4 |

Both failed calls were the same error. The cohort participant opened `stage func:` a second time after `stage anat:`, got `duplicate stage `func``, and merged the blocks.

## What participants wrote

1. **The new path features were found and used where they paid off.** The cohort participant wrote one default rule, `path: derivatives/sub-{sub}[/ses-{ses}][/{@stage}]/{@labels}_{@product}` with `ext: .nii.gz`, in place of the per-product BIDS paths of earlier rounds. It said it would not have found the optional group without the guide's example. The survey participant declared output extensions (`-> Clean .csv`) and still wrote full paths, which the guide says are kept as is. The logs participant wrote explicit path rules and declared no extensions. None used a recipe `root` line: all three kept `data/` in their source rules and rooted at the working folder, which the briefs' output paths call for.
2. **Repeated path rules are no longer the main complaint.** In earlier rounds this was raised in every round. Here none of the three reports mentioned it.
3. **The unmatched-file note alarmed all three.** "`70 files under . match no source rule`" counted the guide, docs and JSON sidecars. Each participant worked out that it was expected, and each still listed it as a problem.
4. **The duplicate-stage error did not say what to do.** It named neither the first block nor the rule that a stage cannot be reopened. The Stages section did not say so either. The report's "printed twice" was two separate calls, `check` and `dag`, not a duplicated message.
5. **`verify` with a `many` input was a guess.** The survey participant wrote `verify fit: validate_panel {waves}`. It worked, but the guide only showed single inputs.

## Changes made in this round

- `duplicate stage` now names the line the stage was first opened on and says to move the lines into that block.
- The unmatched-file note says the files are not read and names one of them, such as `GUIDE.md`.
- The language reference says that a stage is one block that may use products from a later stage, and that `verify` may use a `many` input.

## Next

- If cost allows, rerun s2-cohort with its follow-up, and s6-diagnose, to test `sidecars` and diagnosis messages under the new paths.
- A two-level aggregation (vary date, then server) and a "check before run" pointer to `verify` were each asked for once; neither is acted on yet.
