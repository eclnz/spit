# Useful, readable hovers (#105)

Improve hover information and layout, including but not limited to
redundancy. Declarations explain the generic operation; calls explain the
actual products and types. Keep primitive behavior and command/verification
templates available. Composite calls show typed input/output mappings and
one concrete expansion; declarations retain the generic body. Broken calls
must not claim successfully inferred types.

Format long signatures and signatures with multiple outputs at port
boundaries, preserving types, cardinality, checks, extensions and beside
relationships. Render bodies and expansions as code, and consecutive call
bindings together. Product hovers omit repeated or implicit declared types,
retain inference failures, and bound long consumer lists with a count.
Keep compiler hover fields plain text and editor Markdown untrusted.

- [x] Context-specific information and port-boundary signatures implemented; record the compiler commit after it lands.
- [x] Registration example verified with the binary; five new Rust tests and rendered editor coverage passed. Editor commit `0be3c7e`.
- [x] Guide and architecture updated against verified behavior.
- [x] 596 Rust tests passed (12 existing ignores), 51 editor tests passed (one existing skip), all required checks, docs and Messie passed. Both benchmarks passed; six ordinary outputs and hover metadata unchanged.
- [ ] Create linked PRs on matching branches; compiler merges first.
- [ ] Record completed commits, then delete this plan alone before merge.
