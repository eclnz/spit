# Make DAG counts the default

Issue: #216

## Scope

Make plain human-readable `spit dag` print the existing step-count overview. Keep expanded command output behind `--commands`. Preserve explicit views and combinations, including silent `-o` and stable JSON and written DAG output.

## Steps

- [x] Select counts for an unqualified human-readable invocation and preserve explicit flags.
- [x] Update CLI help, user documentation, and focused behavior coverage.
- [x] Run required checks and commit the implementation.
- [ ] Delete this plan in a standalone commit before merge.
- [ ] Open a PR against `dev`, verify CI, and merge when green.
