# Explain source path failures

`spit check --path-rules` can accept a valid recipe default that does not match the dataset. When `spit inputs` then fails a `require` rule, show the source's effective path rule beside the coverage error and point to unmatched files from the scan. Keep the explanation factual when the dataset simply lacks the source.

## Steps

- [x] Add a command-line diagnostic for a scanned source with zero matches, including its completed path rule and an unmatched-file example when one exists. Commit: "Explain missing source path rules when input coverage fails".
- [x] Let `spit inputs --unmatched` list files even when a `require` rule fails. Commit: "Explain missing source path rules when input coverage fails".
- [x] Cover the failing recipe, update the user guide, and run the required checks. Commit: "Explain missing source path rules when input coverage fails".
- [ ] Delete this plan in its own commit before the work merges.
