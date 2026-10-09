# Multiline operation signatures (#104)

- [x] Logical operation declarations and physical diagnostic locations: 63c24a2.
- [x] Tests for lists, nested types, stages, composite bodies, recovery and editor locations: 63c24a2.
- [x] Language guide layout and example verified against the binary: 63c24a2.
- [x] Companion grammar, extension tests and README: spit-vscode f998fcc.
- [x] Release build, full release suite, clippy, answer keys, tracked-file Messie and pipeline/dataset performance checks pass. Existing example JSON and hovers are byte-for-byte unchanged.
- [x] Linked draft PRs: https://github.com/eclnz/spit/pull/119 and https://github.com/eclnz/spit-vscode/pull/30. Merge SPIT first.
- [ ] Delete this plan in a dedicated final commit before merge.

Layout: continue while operation parentheses are open. Port lines indent beyond the declaration; a closing delimiter may align with it. The body begins only after the complete header ends in `:`. A later declaration is a recovery boundary.

No implementation steps remain deferred.
