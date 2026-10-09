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

- [x] Context-specific information and port-boundary signatures: `5b4b5cd`.
- [x] Five new Rust tests cover registration, primitives, selectors, products
      and broken composites; compiler commit `5b4b5cd`.
- [x] Safe rendered Markdown coverage and matching editor guide: spit-vscode `0be3c7e`.
- [x] Binary-verified guide and architecture: `5b4b5cd`.
- [x] All 596 Rust tests passed (12 existing ignores), 51 editor tests passed
      (one existing skip); required checks, docs and Messie passed for `5b4b5cd`.
- [x] Both benchmarks passed against `2b6569c`; six ordinary outputs unchanged,
      with hover schema, ranges, names, diagnostics and path hints preserved.
- [x] Linked compiler PR #128 and editor PR #33; editor follows #30.
- [ ] Delete this plan in the final commit with recovery instructions.

No implementation step is deferred. The compiler merges before its editor
companion; editor #30 merges before #33 is retargeted to main.
