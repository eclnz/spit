# Report

## 1. Outcome
Produced plan.spitdag (41 jobs: 13 motioncorr, 6 bet, 13 coreg, 6 sessionavg, 3 longitudinal), cohort.spit and cohort.spitin. Confidence 4/5: I checked the printed commands and paths for all job kinds and the run order (run-1,2,3), but I did not diff against an independent expected list.

## 2. Walkthrough
1. Read GUIDE.md; its cohort example (discover + drop + default path with optional groups) matched the task almost exactly, so I wrote cohort.spit and cohort.spitin from it.
2. First check: "error: cohort.spit: line 24, column 7: duplicate stage `func`" (printed twice). I had split func into two blocks to put anat between them. Merged into one func block; SPIT orders stages by dependency and accepted `brain` used before its definition.
3. Then `check`, `dag --commands`, and `dag -o plan.spitdag` all worked first time. Exclusion of sub-03 appeared as a stderr note ("dropped [sub=03] ... found 1").

## 3. Stuck points
None significant. The only stall was the duplicate stage error.

## 4. Guesses
- Naming operations differently from products (register vs coreg) to avoid the same-name warning; the guide confirmed the warning.
- Using stages func/anat only to get {@stage} in the path; guide example confirmed this pattern.
- Source `path` rules written with full BIDS names including extension; guide implies this but doesn't show a bold example.
- Recipe in working folder with no `root` line, since paths start with data/.

## 5. Guide gaps
- Whether a stage may be reopened/split (it can't) is not stated in the Stages section.
- Stage output: not obvious that the stage of `avg` and `long` (long outside stage gives no stage dir) is what yields the required path; I only knew from the cohort path example.
- "note: 91 files under `.` match no source rule" is alarming at first (they are json files, derivatives-free); fine but unexplained.

## 6. Error messages
- "duplicate stage `func`" did not say where the first one was or that stages cannot be reopened. The duplicate print of the message is noise.
- Good: the in-memory notes ("41 jobs resolved: 32 in func, 6 in anat, 3 outside stages", "dropped [sub=03] ...") made verification easy.

## 7. Language friction
Needing the `[...]` optional path group and stage names to reproduce BIDS-like paths is clever but I'd not have found it without the guide example. Single default path plus ext covers everything; no friction beyond that.

## 8. Compared with a shell script, Make, or Snakemake
Faster to a verified result: the session/subject exclusion and numeric run ordering come for free, and the printed commands let me check everything. A shell loop would need special cases for sub-03 and run-3.

## 9. Top three changes
1. Document that a stage can't be split, and improve the "duplicate stage" error to point to the first one.
2. Print each diagnostic once.
3. Explain the "files match no source rule" note (what they typically are) or exclude sidecar json noise.
