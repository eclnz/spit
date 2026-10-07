# Remote continuation

Local work stopped at the user's request on 2026-10-07. Resume from the pushed
branches below; do not assume the documentation checkpoint is merge-ready.

## Documentation

Branch: `codex/documentation-structure` in eclnz/spit.

Read documentation-structure-plan.md for the agreed structure and remaining
work. Guide teaches use, manual specifies valid behavior without guide links,
developer documentation explains implementation. The four user-facing groups
are pipeline (.spit), recipe (.spitin), input inventory (.spitout), and runnable
DAG (.spitdag). Existing document boundaries need not be preserved.

Finish documentation and finalize canonical paths before updating editor links
and the VS Code extension. Existing `src/builtins.rs` URLs and
`tests/builtin_words.rs` still expect docs/language-reference.md; migrate the
links and tests once the manual structure is reviewed. Do not merge this WIP
checkpoint until remaining verification and integration pass.

## Concise hovers

Issue: https://github.com/eclnz/spit/issues/105

Branch: `codex/concise-hovers` in eclnz/spit (separate agent checkout), pushed
at `ea58992` with implementation commit `ef5c74f`.
Read its feature plan and commit for implementation and verification details.
The change removes redundant generic body text at concrete calls, retains the
declaration explanation and concrete expansion, and wraps long signatures.
Extension coverage is deferred until documentation is finalized. Review before
merging and adapt its documentation updates to the new manual/guide structure.

## Readable path hints

Issue: https://github.com/eclnz/spit/issues/106

Investigation only; no implementation branch has changes to push. SPIT JSON
already supplies product, line, and full path, so no schema change is needed.
In spit-vscode, extension.js combines all output paths into one inline string
and provides a generic tooltip. Keep ordinary single-output hints, use a compact
output-name summary for multiple outputs, and provide a Markdown tooltip with
each product and full path separately. Test long paths with meaningful middle
and suffix, several output associations, tooltip escaping, and range filtering.
Implement in an isolated `codex/readable-path-hints` branch after documentation
is finalized. Update extension README and run its relevant tests.

## Multiline operations

Issue: https://github.com/eclnz/spit/issues/104

Discussed as a useful next task; no implementation agent was started. Allow
continuation within input/output lists, preserve nested generic parsing, body
boundaries, and accurate source locations. Coordinate manual and extension
grammar updates. Do not treat it as implemented.

## Cleanup

Keep plans while work remains. Before merging completed work, record completed
steps and delete each feature's plans in a dedicated final commit according to
AGENTS.md. This handoff is a temporary plan, not permanent documentation.
