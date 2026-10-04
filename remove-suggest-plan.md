# Remove `inputs --suggest`

The language asks authors to state sources and paths explicitly. Remove the CLI's inferred source and path rules, while keeping discovery, unmatched-file reporting, and explicit path shapes.

## Steps

- [ ] Remove the flag, its CLI rendering, and the public suggestion API and implementation.
- [ ] Replace the suggestion guide with the explicit source/path workflow; remove feature-only tests and verify the CLI rejects the removed flag.
- [ ] Run format, release build and tests, clippy, answer keys, and Messie; check the binary's help and error text.
- [ ] Record the commits for completed steps here, then delete this plan in its own final commit before merge.
