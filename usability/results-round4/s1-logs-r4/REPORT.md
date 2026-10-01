# Report

## 1. Outcome
Produced plan.spitdag (24 jobs: 20 digests, 3 rollups, 1 fleetsum), logs.spit and logs.spitin. Confidence 5/5: I checked all command lines with `dag --commands`; dates and servers sorted correctly, db1's missing 09-04 handled, .log.gz and .log.1 files and README ignored.

## 2. Walkthrough
1. Read GUIDE.md, wrote logs.spit (source log [server,date], three operations, `@ vary(date)` then `@ vary(server)`, explicit path rules) and a one-line recipe `pipeline logs.spit`.
2. `spit check logs.spitin`, `spit dag logs.spitin --commands`, then `dag -o plan.spitdag`. No errors, no surprises. Only note: "70 files under `.` match no source rule" (expected: docs, examples).

## 3. Stuck points
None.

## 4. Guesses
- That the recipe's folder (the working folder) is the right root, since output paths are relative to it, with the source path `data/logs/{server}/{date}.log`. Confirmed by the output.
- That a no-dimension product (fleet) gets a fixed `path fleet:` rule. Worked.

## 5. Guide gaps
Nothing missing. The "Where files live" section answered the root question.

## 6. Error messages
No errors hit.

## 7. Language friction
None. Whole-path matching of source rules made ignoring .gz/.1/notes automatic.

## 8. Compared with a shell script, Make, or Snakemake
Faster and less error-prone: no listing of servers or dates, and sorting and missing days were handled for me.

## 9. Top three changes
1. A short "aggregate in two levels" example (vary date, then vary server) in the guide front matter.
2. Say explicitly in the guide that source rules match whole paths, so rotated files are ignored (it is in Extensions, easy to miss).
3. `dag -o` could print the job count by operation.
