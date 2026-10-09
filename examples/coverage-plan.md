# Consolidated examples and CLI coverage (#120)

- [x] Combine overlapping selectors patterns and text imports/stages; keep focused stage fixtures in tests. Built in `942e4cf` (27 pipelines become 21).
- [x] Classify every pipeline and recipe; add missing discovery recipes. Built in `942e4cf` (17 entry points, three libraries, one invalid pipeline).
- [x] Check, discover, save inventory, resolve directly/from inventory and save DAG in isolated workspaces. Built in `942e4cf`.
- [x] Assert counts, identities, commands, graph edges and feature relationships; prove failures with negative probes. Built in `942e4cf`.
- [x] Add CI logs/artifacts and document the local command and consolidated catalog. Built in `942e4cf`.
- [x] Run full checks and commit implementation. `942e4cf`: release build/tests, clippy (existing warnings), all usability keys, pinned Messie, strict docs, links and the CLI audit pass. No compiler code or format change; benchmarks and companion changes do not apply.

All implementation is complete; nothing is deferred. Delete this plan in a separate final commit before merging, recording its path and retrieval command in that commit.
