# Product-wide human reviews

[Issue #420](https://github.com/eclnz/spit/issues/420)

- [x] Reject input-level review annotations, preserve input checks and screens, and support a review on the product of a single-output call. Migrate the tests. Implemented in `d3393bd5`.
- [ ] Parse and resolve evidence on the reviewed product. Check evidence references and review dependencies for cycles.
- [ ] Carry human review steps and evidence reads in the saved DAG. Remove input-scoped review targets from the compiler and runner. Schedule evidence for selected analysis and apply verdicts per artifact.
- [ ] Record verdict revisions and report the evidence and verdict destination. Verify the fit-to-QC-to-cohort workflow across pending, pass, rejection, and changed evidence.
- [ ] Migrate the user guide, runnable examples, and VS Code grammar. Verify the compiler, runner, and companion checks together.
