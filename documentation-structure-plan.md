# Documentation structure plan

Build the documentation on `codex/documentation-structure`, starting from `dev`.
Finalize the documentation's organization and canonical URLs before updating
spit-vscode. Coordinate any changed editor links before merging or releasing.

## Intended structure

Reorganize from first principles. Existing filenames, pages, anchors, and the
architecture document do not have to survive. Retain verified knowledge, not
the old layout. Update repository instructions and inbound links to the new
canonical locations rather than keeping obsolete pages solely for compatibility.

- Guide: teach how to use SPIT; link to the manual for precise rules.
- User manual: define valid syntax, behavior, restrictions, and errors without
  linking to the guide.
- Developer documentation: explain the implementation in focused subject pages.

Group the guide and manual around Pipeline (`.spit`), Recipe (`.spitin`),
Input inventory (`.spitout`), and Runnable DAG (`.spitdag`). Keep command-line
behavior in a manual section. Each normative topic has one canonical home.

## Manual specification standard

SPIT is a new language: readers cannot infer its rules from another language.
Define shared terminology, lexical rules, grammar notation, file structure,
name resolution, and the semantic model before relying on them in component
specifications. Distinguish source-level validation from dataset-dependent
resolution and runner obligations.

For each construct document its purpose, allowed files and scope, syntax,
meaning, validity constraints, defaults, interactions, ordering, and failure
conditions. Include minimal valid examples, invalid examples with explanations,
and boundary cases where they clarify behavior. Define observable deterministic
behavior without prescribing incidental implementation details or promising
unstable diagnostic wording.

Specify file formats and CLI contracts completely, including root resolution,
escaping, identities, input/output forms, and versioned DAG fields. Link related
rules within the manual. Teaching sequences belong in the guide.

Maintain a coverage matrix mapping each rule to its canonical manual section
and implementation/test evidence. Use the parser, semantic checks, existing
tests, and actual binary runs together; parsing successfully alone does not
establish a construct's meaning. Record contradictions for resolution rather
than silently turning implementation bugs into language promises. Proposed
features remain in issues until implemented.

## Steps

- [ ] Define terminology, grammar notation, the manual specification template,
  and a behavior coverage matrix; establish navigation from these foundations.
- [ ] Map verified content into the new structure, removing superseded pages
  and duplicated rules without preserving old document boundaries.
- [ ] Start the guide with file roles, direction of flow, and separation of
  concerns, followed by a first working pipeline.
- [ ] Use concept-first headings and a single next-page reading sequence;
  enable previous/next navigation for the documentation site.
- [ ] Consolidate reference and catalog rules into a standalone manual with
  a compact syntax index; remove manual dependencies on teaching pages.
- [ ] Move walkthroughs into the guide, retain an examples index, and shorten
  README to introduction, setup, a quick example, and documentation links.
- [ ] Rebuild useful developer material by subject and add optional expandable detail.
  Keep validity rules visible and DAG format semantics in the manual.
- [ ] Verify behavior against the binary, including the incorrect claim that
  `--root` overrides recipe or inventory roots. Coordinate inventory accuracy
  with https://github.com/eclnz/spit/issues/108 and future recipe boundaries
  with https://github.com/eclnz/spit/issues/107.
- [ ] Finalize canonical documentation paths and anchors. Inventory inbound
  links, especially `src/builtins.rs` and AGENTS.md's architecture references;
  update them to the new structure rather than requiring the old paths to remain.
- [ ] Update editor documentation links and spit-vscode only after the
  documentation structure is finalized; use the same branch name and linked
  PRs for any coordinated changes. Merge SPIT first.
- [ ] Validate strict MkDocs build, links and anchors, guide progression,
  examples, expandable detail rendering, and repository-required checks.
- [ ] Record completed steps with their commits. Before merging, delete this
  plan in a dedicated final commit following AGENTS.md.

## Review basis

The review found overlapping behavioral explanations in README, the guide,
the catalog, the full reference, examples, CLI documentation, and architecture.
The opening guide page introduces artifact families and matching before file
roles. Manual pages link back to guide pages. Architecture mixes implementation
explanations with protocol rules. The existing `--root` override claim contradicts
the CLI, demonstrating the cost of duplicated rules.

Preserve verified knowledge during migration, not documents. Subject boundaries do not
require a separate page for every small topic. Do not document proposed behavior
as current behavior.

## Remote handoff checkpoint

The user requested an immediate push so a remote session can continue. The rewrite
is a work in progress; no completion or merge is claimed. Commit ids will be
recorded by the coordinating agent after the checkpoint is committed.

Completed draft work: guide begins with file flow, roles, and separation of
concerns; a single next-page sequence and Material footer are configured. The
manual is standalone under `docs/manual/`, grouped around the four file kinds,
with foundations, syntax index, matching, checks, types, paths, imports, and CLI.
Former reference/catalog/CLI/DAG pages were consolidated and removed. Example
walkthroughs moved into the guide; README and examples are entry points. Developer
material is split under `docs/developer/` with expandable implementation details.
AGENTS.md points to the new performance and DAG locations.

Pending: expand the topic coverage matrix into rule-by-rule verified evidence;
complete specification grammar, validity/default/interaction/error/boundary
coverage; audit migrated prose for redundancy and teaching-only material; verify
all examples against binary behavior; inspect rendered detail blocks and code
fences; migrate Rust builtin hover URLs and their tests; then coordinate the
VS Code extension on the same branch after documentation is finalized. Inventory
and recipe proposals remain unimplemented.

Canonical hover mappings: products-and-dimensions, dimension-order, stages →
manual/pipeline.md; operations-and-commands → manual/operations.md (selector links
should instead use manual/matching.md); reuse-definitions → manual/reuse.md; paths,
extensions, shapes-on-a-source-placeholder, sidecar-files → manual/paths.md;
recipes, discover-contexts-from-directories, constraints,
exclude-groups-that-meet-a-condition, exclude-named-artifacts → manual/recipe.md;
inputs → manual/inventory.md; checks → manual/checks.md. All former DAG fields
remain in manual/dag.md with their prior anchors.

Verification at checkpoint: parent ran formatting, release build, full release
tests, clippy, and usability keys. Only builtin word documentation-link tests
failed because docs/language-reference.md was intentionally removed and Rust
link integration is deferred; all nine answer keys passed. The first strict
MkDocs build found one migrated guide link and two wrong CLI anchors; those were
corrected before the second build. The second build retained the guide-link warning because its label differed;
the target was then corrected directly. Record the final build result in the
checkpoint commit. Final strict MkDocs build passed after these corrections,
including the parent verification. No binary verification of the rewritten examples has yet
been completed.
