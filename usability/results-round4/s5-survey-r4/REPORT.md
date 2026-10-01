# Report

## 1. Outcome
Produced plan.spitdag (20 jobs: 11 clean, 4 fit, 4 plot, 1 national), survey.spit and survey.spitin. Confidence 4/5: `dag --commands` output matched every command line and path in TASK.md, including numeric wave order (nw: 1, 2, 10) and the .bak file being ignored. Not 5 only because I cannot run the commands and did not diff against a reference.

## 2. Walkthrough
1. Read TASK.md, GUIDE.md (large; needed two reads because tool output was truncated), REPORT.md.
2. Wrote survey.spit and a one-line survey.spitin (`pipeline survey.spit`) in the working folder, with root = working folder so the source path is `data/responses/{region}/wave{wave}.csv` and outputs are `build/...`.
3. `spit check` then `spit dag survey.spitin --commands`: succeeded first time, no errors or warnings. Then `dag -o plan.spitdag`.
Only message seen: "note: 68 files under `.` match no source rule" (expected; includes docs, examples, bin).

## 3. Stuck points
None. The longest part was reading the guide.

## 4. Guesses
- `verify fit: validate_panel {waves}` with a `many` placeholder: guide only shows verify with single inputs; it worked and listed all waves in order.
- `path coefs:` / `path diags:` for the two outputs of one multi-output step (guide says each output has its own product and rule; I assigned `coefs, diags = fit(...)`).
- Writing full paths with extensions plus declaring `.csv/.json/.svg` on operations: guide says a rule ending in the extension it would be given is kept as is; confirmed by output.
- Declared `dimensions [region, wave]` explicitly though sources already order them; harmless.
None turned out wrong.

## 5. Guide gaps
- Whether `verify` gates the fit job in the .spitdag semantic sense is only stated in the Language reference; I did not find in the intro that verify is the intended mechanism for "check before run, don't run if fails" - I inferred it from the wording.
- No explicit statement that `many` placeholders work in `verify` lines.

## 6. Error messages
No errors hit. The note "68 files ... match no source rule" is noise when the recipe sits at a broad root; fine but slightly alarming.

## 7. Language friction
None significant. National table collects over region with `vary(region)`; ordering by region name came from natural sort.

## 8. Compared with a shell script, Make, or Snakemake
About the same or faster: variable wave sets per region and the numeric ordering (wave10) would be fiddly in shell/Make. Preview via --commands made verification easy.

## 9. Top three changes
1. In the guide intro, a short "check before run" recipe pointing to `verify`.
2. State that `many` placeholders are allowed in `verify`.
3. Make the "files match no source rule" note quieter or mention it is normal when the root is the working folder.
