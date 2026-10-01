# Report

## 1. Outcome

Yes. I left `pipeline.spit` and `archive.spitin`, and generated `plan.spitdag` with `spit dag archive.spitin --root data -o plan.spitdag`. Confidence: **5/5**. `dag --commands` showed exactly eight calibrations, eight anomaly jobs, and three station reports. Each report's anomaly arguments were in date order; east had only June 1 and June 3. The three unused source artifacts were old calibration revisions.

## 2. Walkthrough

I read `TASK.md`, `GUIDE.md`, and the blank `REPORT.md`, then listed the archive's paths. I used `reading[station,day]` as the driving family, chose revision 3 with `where(revision=3)`, matched the separately dated baseline with `same(station)`, and collected anomalies with `vary(day)`. I wrote the pipeline and recipe. My first `spit check archive.spitin --path-rules` failed with three messages, including `error: pipeline.spit: line 14, column 45: expected type name` (also lines 18 and 22). I had left the operation output type blank after `->`, thinking an untyped operation could leave it empty. I added explicit types to the sources and operation signatures. The next check said `Recipe valid.` and showed the intended path rules. `spit inputs archive.spitin --root data -o archive.spitout` found 20 source artifacts. `spit dag archive.spitin --root data --commands` showed all 19 commands. It also said `3 source artifacts are used by no job (calibration: 3)`; I identified them as the old revisions selected out by `where(revision=3)`. I then wrote the DAG with `-o`.

## 3. Stuck points

The longest pause was deciding how to join the baseline's `recorded` date to a daily reading without pretending its date matched the reading day. The `same(station)` selector in the guide resolved it. The output-type syntax error was quick to fix.

## 4. Guesses

I first guessed that `operation ... ->` could omit a result type because the guide says types are optional. That was wrong; `spit check` required a type name there. I also assumed a station's single baseline would make `same(station)` unambiguous. The inventory confirmed one baseline per station, and `dag` resolved all jobs.

## 5. Guide gaps

In **Optional types** and **Operations and commands**, I looked for the exact syntax of an entirely untyped operation's return value. I did not find a clear example. The guide's statement that types are optional did not tell me whether `->` itself could be omitted, or whether an operation still needs a named output type. I used explicit types.

## 6. Error messages

`expected type name` pointed to the issue, although a suggestion for a valid untyped operation form would have been more useful. `3 source artifacts are used by no job (calibration: 3)` was helpful: it made the old revisions visible instead of silently ignoring them. `19 jobs resolved` and the full `--commands` listing made the result easy to audit.

## 7. Language friction

Matching the baseline by station alone needed a selector because its archived recording date is independent of the reading day. `baseline @ same(station)` expressed this, but I had to reason carefully about whether the remaining baseline date would be unique. The calibration selector and day collection were concise.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was about the same speed for this small archive, perhaps slightly slower initially because I had to learn its selector and operation syntax. Once written, its plan adapts to new stations or days without editing lists, and `--commands` made the joins and ordering visible before execution. A shell script would need custom matching and sorting logic.

## 9. Top three changes

1. Show a complete untyped operation signature next to the statement that types are optional.
2. Add an example joining a source with an unrelated extra dimension through `same(station)`, including how ambiguity is reported.
3. Make the resolved command listing summarize job counts by operation, so large plans are easier to audit.
