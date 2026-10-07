# Concise operation hovers

- [x] Inspect issue #105 and operation hover generation (ef5c74f).
- [x] Omit the generic body at concrete calls; retain declaration bodies and primitive details (ef5c74f).
- [x] Wrap signatures longer than 100 bytes with one port per line; cover the reported shape (ef5c74f).
- [x] Verify required checks and benchmark both workloads (ef5c74f).
  Pipeline hovers: 32.1 → 31.5 ms; dataset DAG: 172.7 → 169.5 ms. Both regression checks pass.
  Messie reports existing example/fixture findings and generated benchmark folders locally; the clean dev baseline has the same example/fixture findings.
- [ ] Update extension rendering tests after documentation is finalized, as requested.
- [ ] Delete this plan in a dedicated final commit before merge.

## Remote handoff

Implementation: ef5c74f on codex/concise-hovers. Full release tests, release build, clippy, nine usability keys, strict MkDocs build, pipeline and dataset regression benchmarks passed. Byte comparisons passed for inventory and DAG in the same file context and chain check output with JSON and hovers. Clippy emits pre-existing chunks_exact warnings. Local Messie reports existing example/fixture findings (also present on clean dev) and generated benchmark folders.

Resume after documentation structure and URLs are finalized. Create the same branch in spit-vscode and update extension.test.js for a composite call with exactly one concrete expansion, a declaration with its generic body, unchanged primitive information, and multiline signature Markdown. Existing extension.js already uses code blocks for signatures and expands semicolon-separated body steps into separate lines; no renderer change appears necessary. Run extension and grammar tests, coordinate linked pull requests, and do not close #105 until both repositories cover the behavior. Adapt the hover paragraph in docs/language-reference.md to the new manual before merging. Delete this plan only in its dedicated final pre-merge commit.
