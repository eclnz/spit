Three updates from the study team. The working folder and its rules are the same as before.

1. A new subject, `sub-06`, has just been added to `data/` with two sessions.
2. Add a per-session QC report: `qcreport --t1 <brain> --out <out> <coreg> <coreg> ...`, given the session's brain-extracted T1w and all of that session's coregistered runs in run order → `derivatives/sub-<sub>/ses-<ses>/sub-<sub>_ses-<ses>_qc.html`
3. `sub-02` `ses-02` run 3 turned out to be corrupted. Exclude that run from all processing: the session's average and QC report use the remaining runs. The raw file must stay exactly where it is (the archive is read-only), so do not move, rename, or delete it.

Regenerate `plan.spitdag`, overwriting it. Then add a section `## Change request` to the end of `REPORT.md`: for each of the three updates, what you had to change, how long it took compared with the first build, and anything harder than it should have been.
