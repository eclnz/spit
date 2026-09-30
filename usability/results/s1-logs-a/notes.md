REPORT refused. 5/5, compiled and resolved first try. Used --root data.
Biggest uncertainty: guide doesn't say what happens to files matching no source rule; skipped silently.
Bug claim: `dag --paths` printed first job as `note: Job 1`; prefix inconsistency "20 source files verified." with/without note: prefix between --paths and -o runs.
Guesses: ISO date ordering rule; constant path for zero-dim product; pipeline-only recipe legal.
Gaps: unmatched files; many ordering for dates/versions; zero-dim path; minimal recipe; zero-dim display `fleet[]`.
Friction: @drop(date) on operation + @vary(date) on call are redundant (must agree; one could be inferred).
vs: same as shell, bit faster than Snakemake; main cost reading 32KB guide.
Top3: report unmatched files under root; document many ordering; fix `note: Job 1` + minimal recipe + zero-dim example.
