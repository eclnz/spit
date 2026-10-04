# Replace source sidecars blocks with `beside`

Build on `dev` in SPIT and spit-vscode branch `codex/sidecars-beside`. Preserve output `beside` syntax and let source declarations use the same path relationship. Remove the source-only `sidecars` block after its source path, discovery, import, diagnostic, and suggestion behavior has an equivalent.

- [x] Define source `beside` ownership, dimensions, suffix, path precedence, missing-member reporting, and import behavior against current examples. The main source owns the complete path; companions inherit its dimensions and replace its extension. Commit: record after the implementation commit.
- [x] Implement the source model, parser, path binding, recipe path handling, discovery, diagnostics, imports, and suggestions. Commit: record after the implementation commit.
- [x] Replace sidecars examples and documentation and add focused tests. Existing stored output fixtures remain byte for byte unchanged. Commit: record after the implementation commit.
- [x] Update spit-vscode grammar, README, and extension and grammar tests on the matching branch. Commit: record after the extension commit.
- [x] Run format, release build, release tests, clippy, usability keys, Messie, and performance comparisons against the old `dev` commit. Both benchmark checks passed; unchanged benchmark outputs match byte for byte after normalizing the benchmark folder in the DAG root. Commit: record after the implementation commit.
- [ ] Push both branches, link their pull requests, and merge SPIT then the extension into `dev`.
- [ ] Delete this plan in a final plan-only commit before merge.
