# Code audit and refactor plan

Findings from a read-through of `src/` (about 11k lines) plus targeted checks. Line numbers were taken at commit `2bad9af` and will drift; search by function name if they do.

## Baseline

What held at audit time, so a refactor can be checked against it:

- `cargo clippy --all-targets` is clean at default lints; `cargo test` passes.
- No `unsafe`, no `#[allow]`.
- Mutation fuzzing found no panics: the `examples/basic` files, each with one character inserted (`é`, `日`, `😀`, BOM, `# " ' \ { } ( ) [ ] < > @ ,`), deleted or truncated at every position, run through `parse_pipeline`, `diagnose`, `parse_input_spec`, `diagnose_recipe_against`, `parse_source_inventory` (about 15k cases). This is now `tests/mutation_fuzz.rs`, part of the normal `cargo test` (about 5 s in a debug build). Byte-offset slicing is the main panic risk, so it is the safety net for item 1 and any parser change.
- Scale: a 200k-step chain parses and validates (about 1.5 s and 0.9 s, release); 40k source records resolve in about 0.5 s. This is now `tests/scale.rs`, ignored by default: `cargo test --release --test scale -- --ignored`. Its bounds are loose (30 s and 10 s); they catch a stage turning quadratic or recursive, not a slow machine.

**Every item is done only when** clippy and the full test suite (including `mutation_fuzz`) are still clean, and, for items touching `discover`, `compile`, `resolver` or `Pipeline` lookups (2, 18, 21), the scale tests still pass. Behaviour and output text should not change unless an item says so.

## How to re-run the audit

Run from the repo root. The lints are off by default, so they show what a refactor removed or added.

```sh
# Default lints: should stay clean.
cargo clippy --all-targets

# Everything pedantic. Noisy: mostly redundant_pub_crate, must_use and
# missing `# Errors` docs. Count by kind to see movement.
cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery 2>&1 \
  | grep -E '^(warning|error)' | sort | uniq -c | sort -rn

# The structural lints behind items 2, 4, 17 and 18. Library and binary only;
# tests index freely on purpose.
cargo clippy --lib --bins -- \
  -W clippy::too_many_lines -W clippy::cognitive_complexity \
  -W clippy::indexing_slicing -W clippy::string_slice \
  -W clippy::needless_pass_by_value -W clippy::fn_params_excessive_bools \
  -W clippy::map_unwrap_or -W clippy::manual_let_else

# Functions over 60 lines (item 19): clippy's default threshold is 100, so
# lower it with a clippy.toml containing `too-many-lines-threshold = 60`,
# run the too_many_lines lint above, then delete the file.

# Things clippy does not flag:
grep -rnE '\.unwrap\(\)|\.expect\(|unreachable!|as_ptr\(\)' src   # items 1, 3, 4, 6
grep -rn 'feff' src                                                # item 9
```

Dead public API (item 5, 15) is not visible to the compiler because `lib.rs` re-exports it. Check a name with `grep -rw <name> src tests examples` and look for a use other than its definition and its `pub use`.

## Order and dependencies

- Do **1** first: it is the riskiest and the others touch the same files (`span.rs`, `parser/mod.rs`, `discover.rs`).
- **2** before **19** (avoid splitting a function that is about to be rewritten).
- **3** before **5** and **7** (they build on the typed result); **3** also unlocks removing several `expect`s in **4**.
- **12, 13, 14** are one error-type family; do them together, **12** first.
- **15** last of the API work: remove dead items from **5** first, then narrow visibility.
- **19** and **17** last, to avoid churn.
- **21** and **22** are optional and independent.

## Higher priority

### 1. Replace pointer-arithmetic offset recovery with real spans
- **Where:** `src/span.rs` `columns_of` (~143); `src/parser/mod.rs` (~82, ~102, `Focus::Slice`); `src/inputs/discover.rs` (~364).
- **Problem:** offsets are recovered by subtracting `as_ptr() as usize` values. `discover.rs` subtracts unchecked (debug-build panic if the slice is not from that text). Nothing checks `char` boundaries. `Focus::Slice` only works if it is resolved "before the line is dropped".
- **Fix:** parser returns byte spans (or `str::substr_range` where the toolchain allows); delete `Focus::Slice`'s lifetime contract.
- **Done when:** no `as_ptr() as usize` remains in `src/`; diagnostics tests (`tests/diagnostics.rs`, `tests/syntax_errors.rs`, `tests/robustness.rs`) unchanged and passing; fuzz clean.
- **Status: done, with a narrower scope.** A span-carrying parser would touch about 50 `at_token` sites in ten files: the parser works on `&str` slices from `split_once`, `trim` and `strip_prefix`, so offsets must be recovered from slices somewhere, which is what `str::substr_range` does, and that is still unstable (checked on Rust 1.94). What changed instead:
  - `discover.rs` binds values as byte ranges; no address arithmetic, so the unchecked subtraction is gone.
  - `content_columns` computes from lengths; `glued_comment` returns its word's range.
  - The remaining recovery is two helpers in `span.rs`, `address_of` and `columns_at`, the only code that reads an address. `columns_at` rejects an address from other text or one off a char boundary. `Focus::Slice` is now `Focus::Address`, resolved through them.
  - Unit tests in `span.rs`; CLI output compared byte-for-byte with the previous build on the examples and on damaged input.
  - **Follow-up:** when `substr_range` is stable, replace the two helpers with it.

### 2. Break up `discover_source_files` (222 lines)
- **Where:** `src/inputs/discover.rs:41`.
- **Problem:** one function does root validation, pattern building, directory walk, context matching, skip application, file matching, expected-file checks and sorting. Clones the whole `Pipeline` (~45-47) only to extend `product_paths`. The "decode value or record a skip and bail" loop appears twice. `unreachable!` (~97) and `expect("rule binds its dimensions")` (~169) guard something `validate_discovery_rule` proves but the types do not carry. `rank[...]` indexing can panic.
- **Fix:** one function per phase; pass merged source-path rules instead of cloning; validate into a typed `DiscoveryPattern`; shared decode helper.
- **Done when:** no function over ~60 lines in the file; no `unreachable!`/`expect`/map-index in it; `tests/discovery.rs` passes unchanged.
- **Status: done.** `discover` now reads as its phases: `DiscoveryPattern::new`, `source_patterns`, `Listing::of`, `find_contexts`, `skip`, `expected_bindings`, `find_source_files`/`source_record`, `require_source_files`, `sort_records`. `read_binding` is the one decode-or-skip step. `with_source_paths` borrows the pipeline when the recipe sets no source paths; `InputSpec::resolve` merges once and passes the result to both `discover` and `locate_sources`, which no longer clones. Longest function in the file is under 60 lines; no `unreachable!`, `expect`, map indexing or `[..]` slice of `pieces`.
  - Verified by a differential run against the previous commit: 3,000 randomized trees (non-canonical and undecodable values, missing files, files outside contexts, overlapping rules, skips, `require`), comparing `discover_source_files` and `InputSpec::resolve` output and errors, including skip-note order. 0 mismatches; every discovery error path was hit. The harness linked both versions from a scratch worktree, so it is not committed.

### 3. Replace optional-field `Diagnosis` with a typed result
- **Where:** `src/diagnostics.rs` (`Diagnosis`); uses at `main.rs` ~475, ~528, ~623-624 and `diagnostics.rs` ~319, ~406.
- **Problem:** `Diagnosis { diagnostics, pipeline: Option, inventory: Option }` forces `.expect("pipeline passed diagnosis")` at each caller.
- **Fix:** e.g. `Result<Checked, Vec<Diagnostic>>` with warnings carried alongside, so success holds the pipeline/inventory.
- **Done when:** none of those `expect`s remain; JSON diagnostics output byte-identical (editor contract, `tests/json.rs`).

### 4. Remove invariant-guarding `expect`/`unreachable!` in compile and CLI
- `compile/mod.rs` (~84) `unreachable!("ordering only reports cycles")`: make `invocation_order` return a dedicated cycle error.
- `compile/steps.rs` (~53): `step_shape` calls `step_driver`, then `step_context` (which calls `step_driver` again) and `expect("the step has a driver")`. Have `step_context` take the driver. `lower.rs::inferred_dimensions` also calls `step_context`; keep it working.
- `main.rs` (~577) `unreachable!("the command takes one or two files")`: model one-vs-two files as an enum.
- **Done when:** those three panics are gone and the driver is computed once per step.

### 5. Simplify the diagnostics API
- **Where:** `src/diagnostics.rs` ~226-376, re-exported in `lib.rs`.
- **Problem:** seven public entry points. `diagnose_artifacts_at` has no callers; `diagnose_at` and `diagnose_at_with_inputs` are test-only. `diagnose_at_checked(text, source_text, path, Option<&InputSpec>, lenient: bool)` takes a bare bool and two `Option`s. A `RefCell<Option<Pipeline>>` (~243) smuggles a value out of a closure. Redundant `.clone()` at ~288.
- **Fix:** an options struct or `enum Mode { Pipeline, Artifacts }`; the parser closure returns the value instead of the `RefCell`; drop the dead entry point and update tests that use the test-only ones.
- **Done when:** fewer entry points, no `RefCell`, no bare `bool` parameter.

## Redundancy

### 6. Unify duplicated text renderers
- **Where:** `render::write_jobs` (`render.rs` ~98-125) and `spitdag::render_bound_dag` (`spitdag.rs` ~188-231) produce the same Job layout; `render_artifact` and `BoundArtifact::identity` duplicate each other.
- **Also:** about 40 `writeln!(..).unwrap()` on `String` (`render.rs`, `spitdag.rs` ~193-229, `main.rs` ~253-282, `paths/template.rs` ~253); use a helper.
- **Done when:** one job renderer; output text identical (golden-check `spit dag` and `spit dag --paths` on `examples/basic`).

### 7. Remove double work in CLI `prepare()`
- **Where:** `src/main.rs` `prepare` (~614) and `run_inputs`.
- **Problem:** with a recipe, the pipeline is read and diagnosed twice; the settled inventory is rendered to `.spitout` text and re-parsed by the diagnoser; `render_source_inventory(...)` is called identically in `inputs()` and `prepare()`.
- **Done when:** pipeline read and diagnosed once per command; inventory passed directly; CLI output and stderr notes unchanged (`tests/cli.rs`).

### 8. Collapse the three recipe parsers
- **Where:** `src/inputs/mod.rs`: `parse_input_spec`, `parse_input_spec_at`, `parse_recipe_lines` share `pipeline_line` -> `check_input_lines` -> `finish_spec`. Extract the shared step.

### 9. Centralise BOM stripping and duplicated validation helpers
- BOM stripped in `main.rs` (~762) and `imports.rs` (~323) but not in library entry points, so library and CLI differ. Decide where it belongs (likely the library) and do it once.
- "source root is not a directory" duplicated in `paths/bind.rs` (~20) and `inputs/discover.rs` (~51).
- `imports.rs::apply_import` has four copy-pasted "conflicts with existing X" loops.
- **Behaviour change to note:** library parsing of BOM-prefixed text changes; add a test.

### 10. Fix `job_json` cloning and fingerprint coupling
- **Where:** `src/spitdag.rs` `job_json`, `fingerprint`.
- **Problem:** `inputs`/`outputs`/`command`/`verify` Json trees are cloned to build the fingerprint payload, and the FNV hash is over serialized JSON text, so formatting changes silently change fingerprints.
- **Constraint:** fingerprints are part of the `.spitdag` contract. If the hashed representation changes, bump `SPITDAG_VERSION` and say so; otherwise keep values identical. Tests assert known FNV values.

### 11. Deduplicate test helpers
- `spit` is defined 4x across `tests/`; `rendered`, `outputs`, `errors`, `bound` 3x each; `text`, `settle`, `resolve_text` 2x. Move to `tests/support/mod.rs`.

## Rust idiom

### 12. Remove `Deref`/`DerefMut` from `Located<E>`
- `src/span.rs` ~113-127 (Deref polymorphism). Replace with explicit field access/accessors and fix call sites.

### 13. Give `PathError` and `CommandError` distinct types
- Both are `Located<String>` (`paths/template.rs` ~12, `command.rs`), so they mix silently: `bind_dag` `?`s a command error into a path error. Introduce distinct types; keep the public error text.

### 14. Replace stringly and `Box<dyn Error>` library errors
- `InputSpec::check`/`resolve` return `Box<dyn Error>` from `format!().into()`; `ResolveError::InvalidDefinition { detail: String }` carries prose. `main.rs` needs the `Reported` marker and `downcast_ref` as a workaround.
- Introduce a typed `InputError`. Extract the repeated `{operation, output_product, port, product}` group in four `ResolveError` variants into a `PortSite` struct.
- Public API break; coordinate with item 15.

### 15. Narrow the public API and encapsulate fields
- `lib.rs` re-exports about 80 items; `EntityBinding(pub BTreeMap)`, `Pipeline`, `Job` expose all fields (`entities.0.get(..)` throughout).
- Decide the intended public surface first (what does `spit-vscode` or an external backend actually use?). Then `pub(crate)` the rest and add accessors. Clippy `redundant_pub_crate` (~100 hits under pedantic) is style only and not part of this item.

### 16. Make `TypeExpr` `Display` unambiguous
- `src/types.rs`: `Variable("T")` and `Named("T")` both print `T` (`Variable` prints bare when the name has one character). Diagnostics cannot distinguish them; the JSON encoder does. **Output change:** update expected messages in tests and docs.

### 17. Small clippy and idiom findings
- Needless by-value params: `imports.rs:15` (`Place`), `lower.rs:53` (`Step`), `parser/flow.rs:199` (`Option<String>`).
- Identical arms in `parser/source_map.rs` ~188 (`')' if depth == 0` and `',' if depth == 0`).
- Pedantic: `map_or`/`map_or_else` suggestions, `format!` appended to `String`, missing `#[must_use]`.

### 18. Panic-capable indexing
- Where an earlier stage's validation is the only guarantee: `resolver/matching.rs`, `compile/definitions.rs`, `shape.rs`, `resolver/bind.rs` (`dag.product_dimensions[&artifact.product]`, `paths[&artifact.key()]`), `compile/mod.rs` and `resolver/mod.rs` (`shapes[&index]`).
- Use `get()` with a real error, or carry validated data in a typed struct (e.g. store the shape on the compiled step).

## Minor

### 19. Split other long functions (over 60 lines)
`diagnostics.rs` ~376 (94), ~1037 (80), ~880 (77); `error.rs` `Display::fmt` (100); `main.rs` ~302 (80), ~571 (64); `resolver/matching.rs` ~24 (86); `resolver/mod.rs` ~45 (72); `resolver/bind.rs` ~18 (70); `imports.rs` ~12 (85); `inputs/coverage.rs` ~36 (78), ~190 (77); `parser/declarations.rs` ~212 (76), ~343 (88); `parser/sectioned.rs` ~44 (84); `render.rs` ~14 (75); `paths/rules.rs` ~127 (71); `compile/mod.rs` ~62 (69). Regenerate with `clippy::too_many_lines` and `too-many-lines-threshold = 60`.

### 20. Stale references
- `paths/template.rs` ~232 and `docs/language-reference.md` ~167 mention `SPIT_ROOT`, which nothing reads (the dataset root is `--root` / the recipe folder).
- `paths/bind.rs` ~32 comment mentions a `--stage` flag the CLI does not have (only `ResolvedDag::only_stage`, used by `tests/stages.rs`).
- Correct the messages and docs; the message change touches tests that match on it.

### 21. Optional: iterative `invocation_order`, O(1) `is_source`
- `compile/definitions.rs` `invocation_order` is recursive (survived a 200k chain, so theoretical). `Pipeline::is_source` scans all invocations and `source_artifacts` calls it per record (quadratic in principle; 0.5 s at 40k records). Precompute a producer set.

### 22. `spit-vscode`: lexer/parser duplication (separate repo, `eclnz/spit-vscode`)
- `extension.js` re-implements the Rust lexer (`stripComment`, `splitTopLevel`) and a partial declaration parser for semantic highlighting; the two can drift. Options: have `spit check --json` emit token data, or add cross-language conformance tests.
