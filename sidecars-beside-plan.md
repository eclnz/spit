# Replace source sidecars blocks with `beside`

Build on `dev` in SPIT and spit-vscode branch `codex/sidecars-beside`. Preserve output `beside` syntax and let source declarations use the same path relationship. Remove the source-only `sidecars` block after its source path, discovery, import, diagnostic, and suggestion behavior has an equivalent.

- [x] Define source `beside` ownership, dimensions, suffix, path precedence, missing-member reporting, and import behavior against current examples. The main source owns the complete path; companions inherit its dimensions and replace its extension. Commit: `137bd9d`.
- [x] Implement the source model, parser, path binding, recipe path handling, discovery, diagnostics, imports, and suggestions. Commit: `137bd9d`.
- [x] Replace sidecars examples and documentation and add focused tests. Existing stored output fixtures remain byte for byte unchanged. Commit: `137bd9d`.
- [x] Update spit-vscode grammar, README, and extension and grammar tests on the matching branch. Commit: `b8644aa` in spit-vscode.
- [x] Run format, release build, release tests, clippy, usability keys, Messie, and performance comparisons against the original `dev` commit. Both benchmark checks passed; unchanged benchmark outputs match byte for byte after normalizing the benchmark folder in the DAG root. Commit: `137bd9d`.
- [x] Push both branches and link their pull requests: eclnz/spit#79 and eclnz/spit-vscode#20. Commits: `137bd9d` and `b8644aa`.
- [x] Merge the newer `dev` into the SPIT branch. Format, release build, release tests, clippy, usability keys, Messie, extension tests, and both performance comparisons passed. Benchmark outputs match byte for byte after normalizing the benchmark folder in the DAG root. Commit: the merge commit that includes this plan update.
- [ ] Delete this plan in a final plan-only commit, then merge SPIT and the extension into `dev` in that order.
