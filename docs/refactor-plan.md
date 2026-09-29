# Code audit and refactor plan

Findings from a read-through of `src/` (about 11k lines) plus targeted checks. Line numbers were taken at commit `2bad9af` and will drift; search by function name if they do.

## Baseline

What held at audit time, so a refactor can be checked against it:

- `cargo clippy --all-targets` is clean at default lints; `cargo test` passes.
- No `unsafe`, no `#[allow]`.
- Mutation fuzzing found no panics: the `examples/basic` files, each with one character inserted (`é`, `日`, `😀`, BOM, `# " ' \ { } ( ) [ ] < > @ ,`), deleted or truncated, run through `parse_pipeline`, `diagnose`, `parse_input_spec`, `diagnose_recipe_against`, `parse_source_inventory` (about 15k cases). The harness was throwaway; re-create it before touching item 1 or any parser code, since byte-offset slicing is the main panic risk.
- Scale: a 200k-step chain parses and validates (about 1.5 s and 0.9 s, release); 40k source records resolve in about 0.5 s.

**Every item is done only when** clippy and the full test suite are still clean, and, for items touching the parser or spans, the fuzz harness still reports no panics. Behaviour and output text should not change unless an item says so.

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

### 2. Break up `discover_source_files` (222 lines)
- **Where:** `src/inputs/discover.rs:41`.
- **Problem:** one function does root validation, pattern building, directory walk, context matching, skip application, file matching, expected-file checks and sorting. Clones the whole `Pipeline` (~45-47) only to extend `product_paths`. The "decode value or record a skip and bail" loop appears twice. `unreachable!` (~97) and `expect("rule binds its dimensions")` (~169) guard something `validate_discovery_rule` proves but the types do not carry. `rank[...]` indexing can panic.
- **Fix:** one function per phase; pass merged source-path rules instead of cloning; validate into a typed `DiscoveryPattern`; shared decode helper.
- **Done when:** no function over ~60 lines in the file; no `unreachable!`/`expect`/map-index in it; `tests/discovery.rs` passes unchanged.

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
