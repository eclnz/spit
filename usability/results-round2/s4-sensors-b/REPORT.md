# Report

## 1. Outcome

Yes. I wrote `stations.spit`, `stations.spitin`, `stations.spitout`, and `plan.spitdag`. Confidence: 5/5. SPIT resolved 19 jobs: 8 calibration jobs, 8 anomaly jobs, and 3 station reports. I checked the expanded command lines: they use revision 3 only, the station's single baseline, and anomaly files in day order.

## 2. Walkthrough

I read the guide and inspected the file names under `data/`. I defined four sources and three operations in `stations.spit`, then wrote a minimal `stations.spitin` recipe. `spit check` reported `Pipeline valid.` and `Recipe valid.` I ran `spit inputs` with `--root` pointing to `data/`; it reported `note: found 20 source artifacts`. I ran `spit dag ... --commands`, inspected all 19 expanded commands, then wrote the DAG with `spit dag ... -o`.

There were no SPIT errors. The only surprise was `note: 3 source artifacts are used by no job (calibration: 3)`. I checked the file list: those are the older calibration revisions, and excluding them from jobs is intended.

## 3. Stuck points

The baseline files have a date dimension that daily readings do not share. I spent the most time deciding how to join them. The guide's `same(station)` selector resolved it; each station has exactly one baseline file.

## 4. Guesses

I assumed the single baseline present for each station is its reference, regardless of its recorded date. The task description and inventory support that. I also assumed the guide's natural ordering for `many` inputs would order ISO dates correctly; `spit dag --commands` confirmed it. No guess turned out wrong.

## 5. Guide gaps

In “Operations and commands,” I wanted one complete, self-contained example that combines `where`, `same`, and a `many` aggregation. The guide shows the pieces separately and links to examples that are unavailable here. The pieces were enough to complete the task.

## 6. Error messages

There were no error messages. `note: 3 source artifacts are used by no job (calibration: 3)` was useful: it drew attention to the old revisions without blocking the valid plan. `note: 19 jobs resolved.` made the expected count easy to check.

## 7. Language friction

Selecting a calibration revision and a baseline with an unrelated date required two different selectors, `where(revision=3)` and `same(station)`. Once found in the guide, both were concise. I did not need a workaround.

## 8. Compared with a shell script, Make, or Snakemake

SPIT was about the same speed for this small fixed dataset. Its value here was checking the joins and displaying every expanded command before any program ran. A shell script would need explicit loops and careful handling of the missing east day and each station's baseline date.

## 9. Top three changes

1. Put a complete join-and-aggregate example directly in the guide, since linked examples may be unavailable.
2. Have `dag --commands` print a short job count by operation as well as the total.
3. Explain in the `same` selector section that a source with an extra date dimension still needs exactly one matching artifact per station.
