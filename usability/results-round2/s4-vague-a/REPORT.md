# Report

## 1. Outcome

Yes. I wrote `pipeline.spit`, `dataset.spitin`, an optional `dataset.spitout`, and `plan.spitdag`. Confidence: **5/5**. `spit dag --commands` showed 19 jobs: eight calibrations, eight anomaly jobs, and three station reports. The commands use revision 3, the station's own baseline and site file, and anomaly arguments in day order. The three older calibration revisions are unused.

## 2. Walkthrough

I read the guide and listed the archive files. I modeled daily readings by `[station, day]`, calibration by `[station, revision]`, baseline by `[station, recorded]`, and sites by `[station]`. I selected calibration revision 3 with `where(revision=3)`, matched the differently dated baseline with `same(station)`, and collected anomalies with `vary(day)`.

My first `spit check` failed with three errors: "type `calibration` must start with a capital letter", "type `baseline` must start with a capital letter", and "type `site` must start with a capital letter". I had used source product names in operation signatures where type names belonged. I added capitalized source types and used those in the signatures. Both pipeline and recipe then checked as valid.

`spit inputs` found 20 source artifacts. `spit dag --commands` resolved 19 jobs and noted that three calibration artifacts were unused. I inspected all commands, then used `spit dag ... -o plan.spitdag` to write the final plan.

## 3. Stuck points

The type error was the only real stall. The guide's source and operation examples made the distinction clear once I returned to them.

## 4. Guesses

I inferred from the archive and task that the baseline's filename date is merely its recording date and should not equal a reading day. The guide's `same(station)` description supported the join. I also inferred that a future station should have one site, baseline, and approved calibration file; that expectation follows the task, though the recipe does not enforce it independently.

## 5. Guide gaps

In "Selectors" under "Operations and commands", I wanted a complete example combining `where`, `same`, and `vary` in one pipeline. The separate examples were enough, but I had to reason through how their dimensions interacted. In "Where files live", the `--root` explanation was clear.

## 6. Error messages

The three "type ... must start with a capital letter" errors were precise and helped me fix the signatures. The note "3 source artifacts are used by no job (calibration: 3)" was helpful confirmation that older revisions stayed out of the plan. No message misled me.

## 7. Language friction

To use the baseline filed under an unrelated date, I had to preserve its `[recorded]` dimension and use `@ same(station)`. That worked, but the need to put a separate filename-only dimension on a one-per-station input took some thought. I did not find anything impossible to express.

## 8. Compared with a shell script, Make, or Snakemake

SPIT took about the same time for this small archive, mainly because I had to learn its syntax. Its file discovery and generated ordered command list would save work as stations and days are added. A shell script would need explicit join and ordering logic.

## 9. Top three changes

1. Add a complete example with a dated, one-per-group reference joined by `same`, a `where` revision selector, and an ordered `many` report input.
2. Show a short source-product versus type-name example beside operation signatures, including the error caused by a lowercase type.
3. Show the resulting commands directly below an example pipeline, so users can check how selectors and aggregate ordering affect the plan.
