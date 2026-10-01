# Design: speed and cleanup after the paths work

Status: steps 1 and 2 are done.

## Problem

The [paths work](output-paths.md) added extensions, `sidecars`, `beside`, `{x.dir}` and `{x.stem}`, the recipe's `root` line, `@` placeholders, `[...]` groups, `{@labels}` and editor hovers. An audit of `8b174dd..158db65` found that these changes made `spit check` grow with products × steps. It found two bugs and a handful of smells. Every place where time was lost broke a rule the earlier performance work had followed, listed below. The rest of this plan applies those rules to what is left.

Measured with [`profiling/bench.py`](../../profiling/README.md) on a chain of steps, in ms:

| `spit check` | `158db65` | after step 1 |
| --- | --- | --- |
| 1,000 steps | 185 | 18 |
| 4,000 steps | 2,015 | 72 |
| 4,000 steps, `--json --hovers` | 2,717 | 163 |

The dataset commands (`inputs`, `dag`) did not change within noise, on 1,000 and 4,000 subjects with up to 60 extra steps.

## The data-oriented rules

The performance commits that took `dag` over a `.spitout` from 14.5 s to under half a second at 1,000 subjects follow five rules. The first is described in [the architecture notes](../../docs/architecture.md), under the artifact table. New work keeps to them.

1. **One table, columns by id.** A DAG keeps each artifact once in its `Artifacts` table. Jobs refer to artifacts by `ArtifactId`, and what belongs to an artifact, such as its file or its bound path, is a `Vec` indexed by id (`618cf77`, `baba10b`). The rule: don't copy a record into everything that uses it, and don't key per-item data by owned names.
2. **Text interned once, compared by number.** Dimension names and values are `Symbol`s. A binding is a sorted slice of symbol pairs with its hash kept beside it, and equal pairs compare by number (`7018ab4`). The rule: don't compare strings, or build strings, to decide equality.
3. **Group by symbol keys in hash maps.** `EntityBinding::group_key` gives a binding's values for some dimensions as symbol numbers. Groups live in `FxHashMap`s keyed by them, a group's binding is built once, and only the distinct groups are sorted (`155a765`, `54b6c72`). The rule: don't build a projected binding per record, and don't use `BTreeMap`s keyed by bindings.
4. **Find once, then look up by number.** `PathBinder::bind_numbered` finds each product's template and stage once, by its number in the artifact table (`6128583`). The rule: don't search by name inside a loop over products or artifacts.
5. **Do each piece of work once, and copy only when something changes.** `dag` reuses the resolution and the bound paths its diagnosis made (`edda3f7`), and settling copies the inventory only when a rule removes records (`9c2e794`, via `Cow`). The rule: don't redo a stage that another one already did.

**How a change is checked.** Output must be byte-for-byte the same on every example and on generated datasets: `check --json --hovers`, `check --path-rules`, `check --json`, `dag --json`, `dag --paths` and `artifacts`. Times come from `profiling/bench.py`, compared with the commit before via `--old`. Instruction counts come from `bench.py profile` at a fixed size. `tests/scaling.rs` must stay under its growth bound.

## Steps

### 1. Find each product's producer once (done in `9f95a1c`)

Each new path question asked which step makes a product, and each question searched every step: `output_port`, `beside`, `expected_extension`, `added_extension`, `default_extension`, `holder` and `stage_of`. `collect_paths`, `shown_paths` and the hovers asked several of them per product, which broke rule 4.

- `PipelineIndex` (`src/model.rs`) finds each product, operation and producer once, in `FxHashMap`s. The path checks, the editor's paths, path binding, source discovery and label warnings ask it. The `Pipeline` methods keep their answers by searching, which costs no more for a single question.
- Hovers write each product's and operation's text once, not at every mention.
- `check --json` renders only the JSON it prints.

This follows rule 4 for lookups, but the maps are still keyed by product name. Step 3 turns them into columns by product number.

### 2. A dot before the extension (done in `1b8d75c`)

`PathTemplate::extension` reads from the first `.` of the file name. So `sub-{sub}_acq-1.5T.nii.gz` was read as ending in `.5T.nii.gz` and rejected for an operation that writes `.nii.gz`, and the same rule without its extension was rejected too. A rule that ends with the extension it must have now agrees with it (`PathTemplate::ends_with`).

**Still open:** without its extension, `_acq-1.5T` is read as ending in `.5T`, and the error says "drop the extension or use `.nii.gz`". In that case it should say only "use `.nii.gz`". Fix it in `extension_disagreement` (`src/paths/rules.rs`): when the file name has a `.` but no expected extension, leave out "drop the extension".

### 3. Product numbers and per-product columns

**Where it stands.** Two numberings exist side by side.
- `PipelineIndex` maps product names to producers, and `collect_paths` still works out each product's template up to four times: in `PathRule::for_product`, `validate_path_template`, `shown_path` and `added_extension`. Each time it resolves `[...]` groups and copies the template when an extension is added.
- The DAG's `Artifacts` table numbers products in the order it meets them, with its own `product_numbers: FxHashMap<String, u32>`, so `PathBinder` keeps a second numbering beside it.

**The data-oriented form.**
- A product's number is its position in `Pipeline::products`, given at lowering.
- `PipelineIndex` becomes columns by that number: the producer `(step, port)`, the stage, the resolved template `Cow<PathTemplate>` computed once, and the expected and added extensions.
- `collect_paths`, `shown_paths`, `product_details` and `ProductPath::new` read the columns. Names are looked up once, at the edge, where text names a product.
- `Artifacts` takes the pipeline's product numbers rather than making its own, so `PathBinder::bind_numbered` indexes the template column directly and `product_numbers` goes away. If imported or recipe-only products make this awkward, keep a `Vec<u32>` from table number to pipeline number, built once.

**Check.** The same output, and fewer instructions for `bench.py profile` at 1,000 steps and for `dag` at 300 subjects.

### 4. Check the path rules once per command

Plain `spit check` runs `collect_paths` twice: once in `diagnose_checked`, then again in `inspect_paths` (`src/main.rs`, `check`). `check --json --hovers` parses and compiles the document twice: `pipeline_hovers` (`src/editor.rs`) calls `recover_document` and `collect_pipeline` again after the diagnosis did both. Both break rule 5.

- The diagnosis returns the `PathCoverage` it built in `Checked`, as `dag` reuses its diagnosis's resolution through `Records`, and `check` prints and validates that.
- `pipeline_hovers` takes the parsed document and compiled pipeline from the diagnosis when it checked clean. Recovery parsing is kept only for a document with errors, where hovers still explain what parsed.

### 5. Editor paths only when asked

Every `diagnose_checked` builds `Checked::paths` (`shown_paths`, `src/diagnostics.rs`), so `inputs`, `dag` and plain `check` pay for something only `check --json` prints. When the pipeline has no default `path:`, it also copies the whole `Pipeline` just to set one.

- Build the paths when the JSON is written: a function on `Checked`, or a flag in `Context`. `Checked::paths` is a public field, so this changes the library's API, and the commit should say so.
- Pass the built-in default as a fallback template into the template column (step 3) rather than copying the pipeline.

### 6. Sidecar groups by symbol keys

`incomplete_groups` (`src/inputs/mod.rs`) identifies each record by a string, `"dim=value,dim=value"`. It then matches removals by splitting that string on `,` and calling `format!` for every removal × group × dimension. That breaks rules 2 and 3, and it is wrong for a value containing `,` or `=`, which a file name can hold.

- Group a sidecar group's member records by `EntityBinding::group_key(&group.dimensions)` in an `FxHashMap`. Track which members are present as a small bit set over the members' positions in `group.members`, since a group has few members.
- Build each group's binding once, with `project`, and only for groups with a member missing.
- Match a removal by symbols: every pair of its binding is a pair of the group's binding. Add a borrowed helper such as `EntityBinding::within`, beside `matches_shared`. A removal that names a dimension outside the group never matches, as now.
- Sort only the reported groups, as `coverage_gaps` does, so messages come in the order of their bindings.
- Test: a value with `,` and one with `=`, and a recipe with many `exclude` rows over many groups.

### 7. Command arguments without per-job copies

- `ArgPart::Stem` (`src/spitdag.rs`) holds its own `String` copy of the port's extension in every job. The path is known to end with that extension, so the part can hold the length to strip (`Stem { artifact, trim: u8 }`), or the extension can live once per operation port. That follows rule 1, and the commit `618cf77` planned the same for `ArgPart` text.
- `write_command` calls `format!("{{\"{key}\":")` for every `dir` and `stem` part. Push the fixed pieces instead, and split the arm so the key isn't chosen by matching the part again.

### 8. Trust the listing for a file's existence

`require_source_files` (`src/inputs/discover.rs`) checks the sorted listing with `binary_search`, then calls `full.is_file()` for every file. The listing already records each entry's type (`6625ea9`), so the `stat` repeats work the walk has done, and on a network filesystem it is the costly part.

Before dropping it, check how the listing treats symbolic links, so that one to a file still counts and a broken one doesn't. Then measure `inputs` on 4,000 subjects.

### 9. Smaller cleanups

- **A doc comment in the wrong place.** In `src/diagnostics.rs`, `missing_root` sits between `diagnose_recipe_against`'s doc comment and that function. The public function lost its docs, and `missing_root` has two run-together comments. Move it.
- **`shown_path` doesn't escape.** It writes literal text without doubling `{`, `}`, `[` and `]`, unlike `render` in `src/paths/template.rs`. Share the escaping.
- **Hover kinds are strings.** `Hover::kind` is a `String` that holds `product` or `operation`. Make it an enum, or a `&'static str`, and write it as text only in the JSON.
- **`push_str(&format!(…))`.** `operation_signature` and `output_signature` (`src/editor.rs`) build a string to append one. Use `write!`, as clippy's `format_push_string` suggests.
- **A doc comment for two fields.** `OpenGroup` (`src/parser/flow.rs`) describes `header` and `stem` in one comment on `header`. Give each its own.

### 10. Catch a pipeline that grows quadratically

The slowdown in step 1 shipped because no test grows the pipeline. `tests/scaling.rs` grows only the dataset.

- Add a stage to `tests/scaling.rs`, or a test beside it, that diagnoses a chain of 1,000 and 4,000 steps, with paths shown and hovers written. It holds the same growth bound: 4 times as long is in step with the size, 16 is quadratic.
- `tests/scale.rs` is ignored by default and still writes the removed `products:` sections, so both its tests fail when run. Rewrite them in the flow form and run them with `cargo test --release --test scale -- --ignored`.

## Order

Steps 3 to 5 touch the same code, so they go in order: product numbers first, then the work done twice, then lazy editor paths. Steps 6 to 10 are independent and can land in any order. Step 10 may well land first, so that it guards the rest.

Follow the roadmap's rules for every step: one commit each, on the `usability` branch, with its tests and the [checks before every commit](README.md#checks-before-every-commit). For a step that claims a speed-up, the commit gives the `bench.py` numbers before and after, and says that output is byte-for-byte the same.
