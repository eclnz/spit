# Separate input matching, contexts and requirements (#142)

Issue: https://github.com/eclnz/spit/issues/142

Status: implementation complete. Conflict resolution incorporates dev at 455cf63, preserving strict typing, block parsing, entity formatting and inherited roots. Release tests, example checks, Clippy, answer keys and Messie pass; benchmark comparisons against current dev pass and DAG bytes agree. Editor checks are being finalized. This restored plan is deleted in the final dedicated commit after integration.
Mark steps done with commit ids. Delete this temporary plan in its own final
pre-merge commit, with the message and `git show` recovery specified by AGENTS.md.

## Resulting language

A `.spitin` recipe can group dataset-specific input paths in one indented block:

```spit
pipeline analysis.spit
root data

inputs:
    r1: fastq/{sample}_L{lane}_R1.fastq.gz
    r2: fastq/{sample}_L{lane}_R2.fastq.gz
    lut: reference/lut.tsv
    contexts: samples/{sample}

require [sample, lane] where r1 count=1
require [sample, lane] where r2 count=1
require [] where lut count=1
require [sample] where r1 count>=1
```

- A product entry gives an explicit path for a declared source product. Repeated
  product entries are errors. A `default:` entry, at most once, supplies the
  recipe's fallback source path. A pipeline's own source path remains usable for
  standalone `--root` scanning. The precedence between pipeline source paths,
  explicit recipe paths and a recipe default stays as today.
- Each `contexts:` entry gives one directory path pattern. It may match zero,
  one or many directories. Repeating `contexts:` is allowed; matches are
  deduplicated by their bound values and dimension set. Placeholders supply the
  captured dimensions; the recipe does not repeat a name or dimension list.
- Source path entries find existing files or folders and extract bindings.
  Context entries record observed units even when no source file exists. Neither
  kind of entry silently makes another source mandatory or rejects an otherwise
  valid source lying outside a context pattern.
- `require` states the obligation. A source target counts its matching artifacts
  in each observed group. A named source is required only where the grouping
  exists. Group membership comes from source artifacts and context bindings, as
  today. A group with no target source has count zero. No groups to check errors.
- To count directories rather than files, use `contexts[dimensions]` as the
  `where` target, e.g. `require [sample] where contexts[sample,lane] count>=2`.
  It selects context entries whose capture dimensions are exactly the named set,
  counts distinct full bindings after grouping, and unions entries with that
  same dimension set. The target's dimension list selects a domain; it is not a
  second declaration of a path's dimensions. If no such context pattern exists,
  validation errors at the requirement.
- `require [] where lut count=1` is a global requirement: it checks the one
  dimensionless group formed by any observed input or context. The parser must
  now accept `[]` for this purpose. A completely empty inventory still reports
  no groups to check, which is an error rather than success.
- An empty observed directory can form a group and make a source requirement
  fail. An entirely absent unit cannot be named individually without a source,
  directory or other inventory record establishing it. Path captures never
  construct a Cartesian product.
- The pipeline's source declarations or its `dimensions [...]` line determine
  dimension order. Input paths capture values but do not reorder identities,
  collections or `{@entities}`.
- Existing exclusion order remains: named exclusions, conditional exclusions,
  then requirements. A removed group is not checked.

The block is optional: a pipeline with reusable source paths can still scan a
root without recipe-specific paths. `contexts:` is only needed where directories
add observations that source files would miss. Flat monthly files, paired reads,
shared singleton tables and heterogeneous scene products require no directory
statement unless their directories independently establish units.

## Language replacement

Remove `discover name: [dimensions] from dirs pattern`. Do not add an alias or
warning-based transition. Replace recipe-level `path product:` and recipe-level
`path:` with entries in `inputs:`. Keep pipeline path declarations, because they
also specify output paths and support standalone scanning. Replace named
`.spitout` context sections with the already supported unnamed `contexts:` form;
update its rendering and validation so no rule name is required. Existing
`.spitin` recipes, `.spitout` fixtures and examples are migrated in this work.
This is a breaking language change on `dev`; document the migration directly.
The `.spitdag` format does not change.

## Cases the implementation must prove

- Flat monthly files plus one global lookup: source paths find observed months;
  no absent month is invented; a global `require` checks the lookup.
- Paired FASTQ files: an R1-only or R2-only lane forms a group and fails the
  opposite requirement; no context directory is needed.
- Empty sample directory: its context forms a sample group so a required read
  fails. A missing sample directory supplies no sample identity.
- Heterogeneous scene bundle: only products explicitly required by a rule are
  mandatory; an optional temperature product may be absent.
- Per-shot photos, per-visit calibration and global class tables: each product
  keeps its dimensions and path; a requirement targets the intended level.
- Two context paths with equal dimensions: their bindings union and deduplicate.
  Different dimension sets remain distinct for `contexts[...]` counts.
- Existing named and conditional exclusions remove the right groups before
  requirements. Explicit inventory records and source paths moved between the
  pipeline and recipe settle consistently where supported.

## Implementation steps

- [x] Add runnable fixture datasets for the cases above and record baseline
  behavior before changing the parser. Build the previous commit in a worktree
  for the required performance comparisons.
- [x] Add `inputs:` parsing with indented product, `default:` and repeated
  `contexts:` entries. Check duplicate product/default entries, undeclared
  products, invalid placeholders and indentation. Remove `discover` and recipe
  path statements, and update `tests/support/mod.rs::split_rules` for every new
  rule keyword.
- [x] Replace named `DirectoryDiscovery`/`discovered` bookkeeping with context
  patterns and dimension-keyed observed bindings. Source scanning records
  matches; context scanning records directory bindings; neither applies implicit
  completeness. Ensure sorting and output never depend on hash iteration.
- [x] Extend coverage parsing, validation and `require` evaluation for the
  `contexts[dimensions]` target and global `[]` grouping. Preserve other
  source-target `require` rules and `exclude`,
  zero-group diagnostics and settled inventory behavior. Update `.spitout`
  parsing/rendering and stored outputs to use unnamed contexts only.
- [x] Add focused parser, input, resolver and CLI tests for the cases above,
  including no Cartesian product, failed and empty context matches, ordering,
  exclusions and exact context-domain selection. Keep Rust files below 800 lines.
- [x] Update `README.md`, `docs/language-reference.md`, recipe and architecture
  guides, examples, built-in word help, and all changed links. Verify claims
  against the rebuilt binary. Explain old-to-new recipe conversion.
- [x] Update spit-vscode on the same branch name: grammar, built-in keywords,
  hover/diagnostic handling if affected, `extension.test.js`, `grammar.test.js`
  and README. Link the two PRs and merge SPIT first.
- [x] Before each commit run `cargo fmt`, release build/tests/clippy, answer-key
  rebuild and Messie as AGENTS.md requires. Benchmark pipeline and dataset with
  `--old` from the preceding build and verify byte-identical DAG outputs for
  equivalent settled inputs. Review fixture diffs and record benchmark results
  in the commit message.
- [x] Open linked PRs against the development branches. Delete this plan in a
  separate final pre-merge commit, recording what was built and how to recover it.

## Done

Users can see in syntax which paths find files, which directories supply context
and which sources or context counts are required. Ordinary datasets need no
named discovery rule. The four real-data shapes above work; editor support,
examples, docs, tests and stored outputs agree on the new language.

## Validation record

All Rust debug and release tests pass, as do release build, Clippy (with existing chunks_exact warnings), answer-key rebuild and Messie. All 49 editor tests pass. Both required benchmark comparisons pass: pipeline check 15.1 → 13.6 ms, JSON 14.8 → 13.8 ms, hovers 30.7 → 30.3 ms; dataset inputs 75.6 → 70.0 ms, DAG 175.8 → 173.8 ms, check 3.1 → 3.2 ms. Equivalent settled input and root produce byte-identical 72,000-job DAGs. Source fixtures cover the agreed scenarios; the existing field-survey integration fixture covers mixed per-shot, per-visit and global inputs. No intended language behavior is deferred.
