# Product-wide human reviews

[Issue #420](https://github.com/eclnz/spit/issues/420)

- [x] Reject input-level review annotations, preserve input checks and screens, and support a review on the product of a single-output call. Migrate the tests. Implemented in `d3393bd5`.
- [x] Parse evidence on the reviewed product, resolve forward references, and validate dimensions and cycles in the product graph. Add a product selector for calls that make several products. Implemented in `da536b18`.
- [ ] Bind each candidate to its evidence artifact by entities. Check the concrete review dependencies and include the jobs that build evidence for selected analysis. Carry those steps in the saved DAG, replacing the explicit-evidence export diagnostic from `da536b18`.
- [ ] Remove input-scoped review targets. Schedule evidence while ordinary readers wait and apply verdicts per artifact. Use the same review steps for local and Slurm runs.
- [x] Create the verdict CSV as artifacts become ready. Add rows for new cases while preserving existing verdicts and reasons. Implemented in `00050e86`.
- [ ] Record verdict revisions and report the evidence and verdict destination. Verify the fit-to-QC-to-cohort workflow across pending, pass, rejection, and changed evidence.
- [ ] Migrate the user guide, runnable examples, and VS Code grammar. Verify the compiler, runner, and companion checks together.
