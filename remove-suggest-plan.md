# Remove `inputs --suggest`

The language asks authors to state sources and paths explicitly. Remove the CLI's inferred source and path rules, while keeping discovery, unmatched-file reporting, and explicit path shapes.

## Steps

- [x] Remove the flag, its CLI rendering, and the public suggestion API and implementation (`d2801ac`).
- [x] Replace the suggestion guide with the explicit source/path workflow; remove feature-only tests and verify the CLI rejects the removed flag (`d2801ac`).
- [x] Run format, release build and tests, clippy, answer keys, and Messie; check the binary's help and error text (`d2801ac`).
- [ ] Delete this plan in its own final commit before merge.
