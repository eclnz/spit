# Product-wide human reviews

[Issue #420](https://github.com/eclnz/spit/issues/420)

- [x] Reject input-level review annotations, preserve input checks and screens, and support a review on the product of a single-output call. Migrate the tests. Implemented in `d3393bd5`.
- [x] Parse evidence on the reviewed product, resolve forward references, and validate dimensions and cycles in the product graph. Add a product selector for calls that make several products. Implemented in `da536b18`.
- [x] Bind each candidate to its evidence artifact by entities, carry it in the saved DAG, and check the concrete review dependencies. Implemented in `b229bf84`.
- [x] Schedule evidence for selected analysis while ordinary readers wait. Apply verdicts per artifact and verify the same workflow on local and fake Slurm runners. Implemented in `b229bf84`.
- [ ] Remove input-scoped review targets from the compiler and retain compatibility for any supported DAG version that uses them.
- [x] Create the verdict CSV as artifacts become ready. Add rows for new cases while preserving existing verdicts and reasons. Implemented in `00050e86`.
- [ ] Record verdict revisions and report the evidence and verdict destination. Verify the fit-to-QC-to-cohort workflow across pending, pass, rejection, and changed evidence.
- [x] Document the evidence contract and add support in the VS Code grammar. Implemented in `b229bf84` and spit-vscode `d13be37`.
- [ ] Migrate the runnable example and refresh the shared version against dev before merge.
