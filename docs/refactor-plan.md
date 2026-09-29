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
- **Status: done.** `Diagnosis<T = Checked>` is `Result<T, Vec<Diagnostic>>`: `Ok` exactly when no diagnostic is an error, holding `Checked { pipeline, warnings }`; `Err` holds every diagnostic in line order. It is a plain `Result`, so no new sum type. The inventory's presence depended only on whether records were passed, so records have their own entry point, `diagnose_at_checked_with_records`, returning `(Checked, SourceInventory)`; `diagnose_at_checked` lost its records and `lenient` parameters. `recover_parse_errors` returns a `Result`, and the old 94-line `diagnose_with_parser` is now `diagnose_document`, `diagnose_with_records`, `check_document`, `record_diagnostics` and `diagnose_list`. The CLI's `passed` helper prints a diagnosis and returns what it checked.
  - No `expect` remains in `diagnostics.rs` or `main.rs`. CLI stdout, stderr and exit codes compared byte-for-byte with the previous commit over 55 runs: `check` and `check --json` on pipelines and recipes, `inputs`, `dag`, `dag --json` and `artifacts`, with parse errors in both files, resolve errors, lenient mode, a missing file, and every kind of warning. Identical.
  - **For item 5:** this adds one public entry point (8 in all) and leaves the `RefCell` in `located_parser`.

### 4. Remove invariant-guarding `expect`/`unreachable!` in compile and CLI
- `compile/mod.rs` (~84) `unreachable!("ordering only reports cycles")`: make `invocation_order` return a dedicated cycle error.
- `compile/steps.rs` (~53): `step_shape` calls `step_driver`, then `step_context` (which calls `step_driver` again) and `expect("the step has a driver")`. Have `step_context` take the driver. `lower.rs::inferred_dimensions` also calls `step_context`; keep it working.
- `main.rs` (~577) `unreachable!("the command takes one or two files")`: model one-vs-two files as an enum.
- **Done when:** those three panics are gone and the driver is computed once per step.
- **Status: done.** `invocation_order` returns a `Cycle { start, products }`, which gives its own error and the step to report it at, so compile no longer matches a general `ResolveError`. `step_context` takes the driver's groups instead of finding the driver again. The CLI keeps its files as `file` and `second: Option<String>`, so `prepare` matches an `Option` and `check`/`inputs` no longer index. CLI output unchanged, including arity errors; added a test that a cycle is reported once, at the step it was found at.

### 5. Simplify the diagnostics API
- **Where:** `src/diagnostics.rs` ~226-376, re-exported in `lib.rs`.
- **Problem:** seven public entry points (eight after item 3). `diagnose_artifacts_at` has no callers; `diagnose_at` and `diagnose_at_with_inputs` are test-only. `diagnose_at_checked(text, source_text, path, Option<&InputSpec>, lenient: bool)` takes a bare bool and two `Option`s. A `RefCell<Option<Pipeline>>` (~243) smuggles a value out of a closure. Redundant `.clone()` at ~288.
- **Fix:** an options struct or `enum Mode { Pipeline, Artifacts }`; the parser closure returns the value instead of the `RefCell`; drop the dead entry point and update tests that use the test-only ones.
- **Done when:** fewer entry points, no `RefCell`, no bare `bool` parameter.
- **Status: done.** A `Context { path, recipe, lenient }` (with `Context::at(path)`) replaces the positional path, recipe and bool. Six entry points instead of eight: `diagnose` and `diagnose_in` return every diagnostic; `diagnose_checked` and `diagnose_checked_with_records` return a `Diagnosis`; `diagnose_recipe` and `diagnose_recipe_against` are unchanged. Removed `diagnose_at`, `diagnose_at_with_inputs`, `diagnose_artifacts_at` (no callers), `diagnose_at_checked` and `diagnose_at_checked_with_records`. `Context::parse` returns the pipeline as written beside the document, so the `RefCell` is gone; the recipe check stays inside the parser, so error recovery is as before. `lenient` remains, as a named field of `Context`. CLI output unchanged.

## Redundancy

### 6. Unify duplicated text renderers
- **Where:** `render::write_jobs` (`render.rs` ~98-125) and `spitdag::render_bound_dag` (`spitdag.rs` ~188-231) produce the same Job layout; `render_artifact` and `BoundArtifact::identity` duplicate each other.
- **Also:** about 40 `writeln!(..).unwrap()` on `String` (`render.rs`, `spitdag.rs` ~193-229, `main.rs` ~253-282, `paths/template.rs` ~253); use a helper.
- **Done when:** one job renderer; output text identical (golden-check `spit dag` and `spit dag --paths` on `examples/basic`).
- **Status: done.** `render_dag` and `render_bound_dag` (moved to `render.rs`) both map their DAG onto one `JobText` view with one `Display`; the only difference is whether lines carry a port and a path. The artifact identity format is one function, `model::identity`, used by both and by `BoundArtifact::identity` (in `model`, since `spitdag` is a shared module and may not depend on `render`; `tests/architecture.rs` enforces this). The artifacts report and the CLI help are `Display` types too, and `encode_component` builds its string without `write!`, so no `writeln!(..).unwrap()` remains in `src/`. CLI output unchanged, including every `help` page and the artifacts report with incomplete jobs and coverage gaps.

### 7. Remove double work in CLI `prepare()`
- **Where:** `src/main.rs` `prepare` (~614) and `run_inputs`.
- **Problem:** with a recipe, the pipeline is read and diagnosed twice; the settled inventory is rendered to `.spitout` text and re-parsed by the diagnoser; `render_source_inventory(...)` is called identically in `inputs()` and `prepare()`.
- **Done when:** pipeline read and diagnosed once per command; inventory passed directly; CLI output and stderr notes unchanged (`tests/cli.rs`).
- **Status: done, with a narrower scope.** Found while doing it: `dag`/`artifacts` on a recipe printed every pipeline warning twice, once per diagnosis. `run_inputs` is now `load_recipe` (read and check the pipeline once, printing only when it fails) and `settle` (resolve the recipe once). `spit inputs` prints the pipeline's warnings itself; `dag`/`artifacts` print the one records diagnosis, which includes them. `prepare` reuses the pipeline text and the settled inputs instead of reading the file and resolving the recipe again. **Output change:** the duplicated warnings are gone; nothing else differs (golden diff is only those lines). New test `a_recipe_run_in_memory_prints_each_pipeline_warning_once`, which fails on the old build.
  - Reusing the settled inputs was checked on the 3,000 random trees: settling once equals settle, render, parse, settle again (inventory for jobs, unavailable sources, gaps) in all 752 cases where the second settle succeeds.
  - **Not done:** the pipeline is still parsed and checked twice internally (alone, to discover sources; then with the records), and the settled inventory still goes through `.spitout` text, because record errors are located in that text. Doing either needs a two-phase diagnosis API (check a pipeline, then records against it) that `Checked` would carry its parsed document for.
  - **Pre-existing bug found (not fixed):** in the other 158 cases the second settle refuses what the first accepted. When a `skip` rule removes every context of a discovery that a `require` rule names, the rendered `.spitout` has no section for that discovery, and reading it back fails with "coverage rule for discovery `…` needs named contexts in the inventory". So `spit dag x.spitin` can fail where `spit inputs x.spitin` succeeds. Before and after this change alike.

### 8. Collapse the three recipe parsers
- **Where:** `src/inputs/mod.rs`: `parse_input_spec`, `parse_input_spec_at`, `parse_recipe_lines` share `pipeline_line` -> `check_input_lines` -> `finish_spec`. Extract the shared step.
- **Status: done.** `parse_recipe` does `pipeline_line`, `check_input_lines` and the parse, given how to parse; `finish_spec` takes the pipeline file. `parse_input_spec` is `parse_recipe_lines` without the lines, and `parse_input_spec_at` only adds the located parse and the folder. Behaviour unchanged.

### 9. Centralise BOM stripping and duplicated validation helpers
- BOM stripped in `main.rs` (~762) and `imports.rs` (~323) but not in library entry points, so library and CLI differ. Decide where it belongs (likely the library) and do it once.
- "source root is not a directory" duplicated in `paths/bind.rs` (~20) and `inputs/discover.rs` (~51).
- `imports.rs::apply_import` has four copy-pasted "conflicts with existing X" loops.
- **Behaviour change to note:** library parsing of BOM-prefixed text changes; add a test.
- **Status: done.** `parser::without_bom` is the one helper. Every public entry point that takes a document's text strips it (`parse_pipeline`, `parse_pipeline_at` and every located parse via `parse_located_document`, the three recipe parsers via `parse_recipe`, `parse_source_inventory`, `diagnose_checked`, `diagnose_checked_with_records`, `diagnose_recipe`, `diagnose_recipe_against`), and `Diagnostic::line_text` strips too, so `display_in`, `utf16_columns` and `render_diagnostics_json` count columns the same way. The CLI no longer strips on its own. **Bug fixed:** `spit check r.spitin` rejected a pipeline file saved with a BOM that `spit check p.spit` and `spit inputs r.spitin` accepted, because `diagnose_recipe` read the file without stripping. Tests: `a_byte_order_mark_is_ignored_by_every_entry_point` (including line-1 error columns), `a_recipe_checks_a_pipeline_saved_with_a_byte_order_mark`.
  - `paths::require_directory` is the one "source root is not a directory" check.
  - `apply_import` checks conflicts through `defined(pipeline)`, one list per kind, instead of four loops (85 to 50 lines). No test covered those messages; `an_import_may_not_define_again_what_the_file_defines` now does, and all four were compared with the old build.

### 10. Fix `job_json` cloning and fingerprint coupling
- **Where:** `src/spitdag.rs` `job_json`, `fingerprint`.
- **Problem:** `inputs`/`outputs`/`command`/`verify` Json trees are cloned to build the fingerprint payload, and the FNV hash is over serialized JSON text, so formatting changes silently change fingerprints.
- **Constraint:** fingerprints are part of the `.spitdag` contract. If the hashed representation changes, bump `SPITDAG_VERSION` and say so; otherwise keep values identical. Tests assert known FNV values.
- **Status: done; fingerprints unchanged.** The work payload is a borrowed `json::ObjectRef` over the job's own values, written by the same `write_object` as `Json::Object`, so the four deep clones are gone and the hashed bytes are identical. The coupling to the JSON format is now explicit rather than silent: `fingerprint` documents it, and a test pins a whole job's fingerprint (`72f6d8ecbacfd9ad`, taken from the previous commit's code), so a change to the writer fails a test instead of changing every fingerprint. No `SPITDAG_VERSION` bump. `dag --json` output identical.

### 11. Deduplicate test helpers
- `spit` is defined 4x across `tests/`; `rendered`, `outputs`, `errors`, `bound` 3x each; `text`, `settle`, `resolve_text` 2x. Move to `tests/support/mod.rs`.
- **Status: done.** Only helpers with identical bodies moved to `tests/support/mod.rs`: `bound`, `errors`, `outputs` (3 copies each), `rendered`, `text` and `spit` (2 each; the `spit` in `cli.rs` and `discovery.rs` differed only in how it spelled `Output`). Same-named helpers that do different things stay local: `spit` in `robustness.rs` (takes stdin) and `stages.rs` (returns a tuple), `rendered` in `robustness.rs` (with columns), `settle` and `resolve_text`. 247 tests before and after.

## Rust idiom

### 12. Remove `Deref`/`DerefMut` from `Located<E>`
- `src/span.rs` ~113-127 (Deref polymorphism). Replace with explicit field access/accessors and fix call sites.
- **Status: done.** The impls are gone. The library leaned on them in five places, all for a `ParseError`'s `kind` or `message`: `ParseError::kind()` reads the kind, `with_kind` sets it as a builder, and `message()` already existed. **Public API change:** callers write `error.kind()` and `error.message()` instead of reaching `ParseFailure`'s fields through `Deref`; 14 test lines changed accordingly (fixed only where the compiler flagged them, since `Diagnostic` has a real `message` field).

### 13. Give `PathError` and `CommandError` distinct types
- Both are `Located<String>` (`paths/template.rs` ~12, `command.rs`), so they mix silently: `bind_dag` `?`s a command error into a path error. Introduce distinct types; keep the public error text.
- **Status: done.** `PathError = Located<PathProblem>` and `CommandError = Located<CommandProblem>`; each problem is a message newtype declared with a small `span::message_error!` macro (the two would otherwise be identical boilerplate), and `Located::new` is generic over `E: From<String>`. The compiler then found the two places they mixed: diagnostics chained both into one list (now mapped separately), and `bind_dag` returned command errors as path errors. **Public API change:** `bind_dag` returns `BindError { Path, Command, Dag }`; its "job lacks `{placeholder}`" error is a `Command` error, and the two DAG-does-not-match-its-pipeline errors are `Dag`. `Display` text is unchanged; CLI output identical.

### 14. Replace stringly and `Box<dyn Error>` library errors
- `InputSpec::check`/`resolve` return `Box<dyn Error>` from `format!().into()`; `ResolveError::InvalidDefinition { detail: String }` carries prose. `main.rs` needs the `Reported` marker and `downcast_ref` as a workaround.
- Introduce a typed `InputError`. Extract the repeated `{operation, output_product, port, product}` group in four `ResolveError` variants into a `PortSite` struct.
- Public API break; coordinate with item 15.
- **Status: done, with a narrower scope.** `InputSpec::check` and `resolve` return `InputError { Resolve, Path, NotASource, PathInBoth, NoDiscoveryPath }` instead of `Box<dyn Error>`, so `diagnose_recipe_against` matches `InputError::Resolve` instead of `downcast_ref`, and so does a test that had to `downcast`. No `Box<dyn Error>` remains in the library. `PortSite { operation, output_product, port, product }` replaces the repeated fields of `TypeMismatch`, `TypeVariableConflict`, `MissingInput`, `AmbiguousInput` and `CollectionTooSmall` (which gains the product bound to its port, and boxes its context like the others); diagnostics match them with one exhaustive arm each. Messages unchanged; new test `a_recipe_that_does_not_fit_its_pipeline_says_why` pins the three recipe messages, compared with the old build through `check`, `inputs` and `dag`.
  - **Left as is:** `InvalidDefinition { subject, detail }` and `UnsupportedShapeRelationship { operation, detail }` keep prose details (19 and 19 construction sites). Their `subject`/`operation` is the machine-readable part diagnostics use; structuring every detail would add about 40 variants and change nothing callers can do. The CLI's `Reported` marker and `is::<Reported>()` stay: the binary uses `Box<dyn Error>` as glue, which is ordinary for a `main`.

### 15. Narrow the public API and encapsulate fields
- `lib.rs` re-exports about 80 items; `EntityBinding(pub BTreeMap)`, `Pipeline`, `Job` expose all fields (`entities.0.get(..)` throughout).
- Decide the intended public surface first (what does `spit-vscode` or an external backend actually use?). Then `pub(crate)` the rest and add accessors. Clippy `redundant_pub_crate` (~100 hits under pedantic) is style only and not part of this item.
- **Status: done, with a narrower scope, by decision.** Nothing outside this repo links the library: `spit-vscode` runs the CLI, and a backend reads the `.spitdag` JSON. Of the 95 re-exports, 29 are unused outside the library, but nearly all appear in public signatures or fields (`BoundDag.jobs` holds `BoundJob`, `ResolveError` holds `DefinitionSubject`, and so on), so dropping their re-exports would leave public types callers cannot name. What changed:
  - `EntityBinding`'s map is private, behind `get` (returning `&str`), `binds`, `iter`, `dimensions`, `len`, `is_empty`, crate-private `extend`, and `From<BTreeMap>`/`FromIterator` to build one. It was the type reached into most (12 places in `src`, 9 in tests).
  - `DefaultPort` is crate-private (in no public signature). Doing so exposed a dead variant, `DefaultPort::Output`, now removed.
  - **Left as is:** `Pipeline`, `Job`, `OperationDef` and the other model types keep public fields. They are a plain data model the tests build as struct literals; hiding them would need builders for every type and change nothing for a caller. Revisit if the crate is ever published for other Rust code.

### 16. Make `TypeExpr` `Display` unambiguous
- `src/types.rs`: `Variable("T")` and `Named("T")` both print `T` (`Variable` prints bare when the name has one character). Diagnostics cannot distinguish them; the JSON encoder does. **Output change:** update expected messages in tests and docs.
- **Status: done.** A variable always prints with its `$` marker, which the parser accepts in signatures, so each type reads back as itself where it can be written. **Output change:** for example `type mismatch at \`f.input\`: product \`raw\` is T, expected List<T>` (the second `T` a variable) now reads `... expected List<$T>`. No existing test, golden output or doc depended on the old form; new test `a_type_variable_prints_marked_so_it_differs_from_a_named_type` pins the round trip and that message.

### 17. Small clippy and idiom findings
- Needless by-value params: `imports.rs:15` (`Place`), `lower.rs:53` (`Step`), `parser/flow.rs:199` (`Option<String>`).
- Identical arms in `parser/source_map.rs` ~188 (`')' if depth == 0` and `',' if depth == 0`).
- Pedantic: `map_or`/`map_or_else` suggestions, `format!` appended to `String`, missing `#[must_use]`.

### 18. Panic-capable indexing
- Where an earlier stage's validation is the only guarantee: `resolver/matching.rs`, `compile/definitions.rs`, `shape.rs`, `resolver/bind.rs` (`dag.product_dimensions[&artifact.product]`, `paths[&artifact.key()]`), `compile/mod.rs` and `resolver/mod.rs` (`shapes[&index]`).
- Use `get()` with a real error, or carry validated data in a typed struct (e.g. store the shape on the compiled step).
- **Status: done for indexing that relies on another stage.** `compile` returns `CompiledPipeline { steps }`, each `CompiledStep` holding its invocation, operation, output products with their inferred types, and shape, in dependency order; the resolver iterates them, so its five map and vector lookups (`invocations[index]`, `operations[..]`, `products[..]`, `inferred_types[..]`, `shapes[&index]`) are gone. `bind_dag` looks up each artifact's dimensions and path with `get`, so a hand-built `ResolvedDag` that lacks one gets `BindError::Dag` instead of a panic; command expansion does the same for paths and a many input. `Job::output` documents its panic (every resolved job has an output). Output unchanged; scale tests pass.
  - **Left as is (42 sites clippy lists):** indices taken from `enumerate`/`position` over the same collection a line or two above, a step's shape indexing its own inputs (now held together in `CompiledStep`), the cycle search's own bookkeeping in `compile/definitions.rs`, and bounds-checked byte loops. Converting them to `get` would add error paths that cannot be reached.

## Minor

### 19. Split other long functions (over 60 lines)
`diagnostics.rs` ~376 (94), ~1037 (80), ~880 (77); `error.rs` `Display::fmt` (100); `main.rs` ~302 (80), ~571 (64); `resolver/matching.rs` ~24 (86); `resolver/mod.rs` ~45 (72); `resolver/bind.rs` ~18 (70); `imports.rs` ~12 (85); `inputs/coverage.rs` ~36 (78), ~190 (77); `parser/declarations.rs` ~212 (76), ~343 (88); `parser/sectioned.rs` ~44 (84); `render.rs` ~14 (75); `paths/rules.rs` ~127 (71); `compile/mod.rs` ~62 (69). Regenerate with `clippy::too_many_lines` and `too-many-lines-threshold = 60`.

**Status: done; 24 functions over 60 lines down to 6, which are left by decision.** Split where a function had separable phases: `expand_step`/`expand_job`, `bind_dag` via a `Binder`, `apply_skips` into `rejected_groups`/`remove_groups` (with `CountRequirement::allows` and `missing_values` shared with `coverage_gaps`, which had copies of both, and `group_errors`), `parse_binding` (`parse_pins`, `parse_each`), `parse_coverage_rule` (`parse_rule_terms`, `parse_group_dimensions`), `parse_args` (`Flags::add`, `Flags::check_conflicts`, `take_files`), `prepare` (`prepare_recipe`), `sectioned_line` (`section_statement`), `error_location` (`pipeline_place`, `inventory_place`), `collect_paths` (`PathRule::for_product`) and `bind_path` (`entities_component`). The sectioned and flow parsers now share `StatementKind::{product, operation, constraint, command}`, which each built with the same code. Items 2, 3 and 4 had already split the three longest.

Left over 60: `ResolveError`'s `Display` (112: one flat arm per variant, clearest in one place), and five at 61–65 lines (`check_selectors`, `render_source_inventory`, `empty_step_warnings`, `collect_pipeline`, `comma_items`) that are a list of checks or one state machine, where a split would move code without clarifying it.

**Bugs found while splitting, both fixed with tests:**
- `empty_step_warnings` followed every path from a step back to its sources, so a chain of steps each reading the previous one twice took exponential time (14 s at 24 steps, hours at 40). Each product's missing sources are now found once. Test: `steps_that_read_a_product_twice_are_checked_in_linear_time`.
- Type arguments nested about 10,000 deep (`A<A<...>>`) crashed `spit check` with a stack overflow, since parsing and unification recurse per level. Nesting past 64 levels is now a parse error. Test: `deeply_nested_type_arguments_are_an_error_not_a_crash`.

### 20. Stale references
- `paths/template.rs` ~232 and `docs/language-reference.md` ~167 mention `SPIT_ROOT`, which nothing reads (the dataset root is `--root` / the recipe folder).
- `paths/bind.rs` ~32 comment mentions a `--stage` flag the CLI does not have (only `ResolvedDag::only_stage`, used by `tests/stages.rs`).
- Correct the messages and docs; the message change touches tests that match on it.
- **Status: done.** The absolute-path error now says "must be relative to the dataset root, not start with `/`", and the language reference says paths are relative to "the dataset root: the recipe's folder, or `--root` when given", matching how the rest of the docs name it. The `--stage` comment now names `ResolvedDag::only_stage`. **Output change:** that one error message; its test updated. Nothing in `spit-vscode` mentioned `SPIT_ROOT`.

### 21. Optional: iterative `invocation_order`, O(1) `is_source`
- `compile/definitions.rs` `invocation_order` is recursive (survived a 200k chain, so theoretical). `Pipeline::is_source` scans all invocations and `source_artifacts` calls it per record (quadratic in principle; 0.5 s at 40k records). Precompute a producer set.
- **Status: done.** Not only theoretical: a new unit test ordering a 100k-step chain on a test thread (2 MB stack) overflowed the recursive `invocation_order`. It is now a depth-first search with an explicit stack of (step, next input) frames, visiting in the same order and reporting a cycle from the same step. Compared with the old code on 8,000 random step graphs (3,454 with cycles, 4,000 acyclic with shuffled declaration order): diagnostics and job order identical in all. `source_artifacts` builds its name-to-product map and set of produced products once instead of scanning per record (the first declaration of a name still wins).

### 22. `spit-vscode`: lexer/parser duplication (separate repo, `eclnz/spit-vscode`)
- `extension.js` re-implements the Rust lexer (`stripComment`, `splitTopLevel`) and a partial declaration parser for semantic highlighting; the two can drift. Options: have `spit check --json` emit token data, or add cross-language conformance tests.
