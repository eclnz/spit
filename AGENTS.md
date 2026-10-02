# Notes for agents

## Plan files

Write plans in Markdown while a feature is being built: designs, roadmaps, audits, lists of steps. They help the work, and they help whoever picks it up next. But once a feature is done, a plan is clutter. Plans pile up, they go stale, and nobody knows whether they can be deleted. So a plan lives only as long as the work it plans. Once the work is merged, git keeps the plan, not the tree.

**While the work goes on.** Keep a plan beside the work it plans, and keep it current. Mark steps done as they land, with the commit that did each one.

**Before the feature merges.** On the feature's branch, the last commit before the merge deletes its plan files. The commit does nothing else, and its message records what was deleted:

- the title names the plan, as in "Delete the output paths plan, now that the paths work is built";
- the path of each deleted file;
- what the plan set out to do, and what was built;
- any step left undone, and why. Steps still worth doing go in a GitHub issue, which the message links to. Don't keep the plan file for them;
- how to read the plan again: `git show <this commit>^:<path>`.

**Finding a deleted plan.** Search the log, not the tree:

```sh
git log --diff-filter=D --name-only --format='%h %s' -- '*.md'   # every deleted Markdown file
git log --grep='plan' --format='%h %s'                           # commits that name a plan
git show <commit>^:<path>                                        # the plan as it was before deletion
```

**What is not a plan.** These stay in the tree:

- The user-facing docs: `README.md` and `docs/`.
- How the code works now. That belongs in `docs/architecture.md`, so move it there before deleting a plan that explains it.
- Tools that are still used, such as `usability/harness` and `profiling/`.
- Records of what happened, such as the usability study's rounds, archived in `usability/rounds.zip`.

**Links.** Permanent files (code, docs, tests and other notes) don't link to a plan, because the link breaks when the plan goes. Name the plan's path as plain text instead, which `git log -- <path>` finds, or cite the commit that deleted it.

## Archives

Records and data that people rarely read, and that only a tool needs as files, are kept as zip archives rather than loose files, so they don't bloat the tree. Examples are a study round's reports and runs, and the harness's scenarios. Pack them with `usability/harness/archive.py pack <folder> <archive.zip>`: it sorts the entries and fixes their timestamps, so the same files always give the same archive. A script that needs the files unpacks them to a temporary folder. The loose files stay in the git history.

## Writing the code

SPIT is a compiler, so most of its code is data being turned into other data: text into statements, statements into a `Pipeline`, a pipeline and an inventory into a DAG, a DAG into a `.spitdag`. The code is laid out around that data, not around objects that hide it. These rules describe how the code is written now. A change that breaks one should say why in its commit message.

Some rules are checked, not just written down. `Cargo.toml` forbids `unsafe` and turns on clippy's `disallowed_types`, which `clippy.toml` sets to the standard library's `HashMap` and `HashSet` with their default hasher, and to `Rc`, `RefCell` and `Cell`. `src/lib.rs` and `src/main.rs` deny `unwrap` outside tests. `tests/architecture.rs` checks file length and the boundaries between steps. `cargo clippy` and `cargo test` run these checks, so the checks before every commit cover them.

The five rules under [Performance](docs/architecture.md#performance) come first: one table with columns by id, text interned once, grouping by symbol keys, finding once and then looking up, and doing each piece of work once. The rules below are how the rest of the code keeps to them.

**Data and the functions over it**

- **Plain data, open fields.** A stage's input and output are structs and enums with public fields, such as `Pipeline`, `Job` and `ResolvedDag`. Add a method when it keeps an invariant that open fields can't, as `Artifacts` does with its columns. Don't add a getter or a builder for a field anyone may set.
- **Enums and `match`, not traits.** A closed set of cases is an enum, and code that handles them matches on it, so the compiler finds every place a new case touches. The crate has one trait, `Out` in `src/json.rs`, which lets the JSON writer write either to a string or to a hasher. Don't add a trait with one implementation, and don't use `dyn` outside `main.rs` unless it saves code.
- **Stages are functions.** A stage takes what it reads by reference and returns what it makes, as `resolve(&pipeline, &inventory)` does. It keeps no state between calls and holds no handle to an earlier stage.

**Ownership and identity**

- **One owner, and ids or borrows everywhere else.** A table owns its records. Anything else that needs a record holds its id (`ArtifactId`) or borrows it (`Artifact<'a>`). There is no `Rc`, `RefCell` or other shared mutable state, and clippy rejects them.
- **Ids are newtypes over `u32`.** An id wraps a `u32` so that ids of different tables can't be mixed up, and it turns into an index only at the table (`ArtifactId::index`). Job ids are still a bare `usize`, which new code shouldn't copy. Convert a length into an id with `u32::try_from(..).expect(..)`, and give the limit in the message, as in "fewer than 2^32 artifacts in a DAG".
- **Copy a handle, not a record.** When a value has many holders, share it behind one pointer, as `EntityBinding` does with an `Arc`. Return a `Cow` when a stage usually passes its input through unchanged (`Pipeline::path_template_for`). A `.clone()` of a `String`, a `Vec` or a map inside a loop over artifacts or jobs is a bug unless the commit says why.
- **Owned text is fine on cold paths.** Errors, diagnostics and the parser's output may hold `String`s. Box a large field of an error variant, as `ResolveError` does, so that `Result` stays small on the path that succeeds.

**Collections and order**

- **Lookups use `FxHashMap` and `FxHashSet`**, from `rustc-hash`. They are the standard `HashMap` and `HashSet` with the Fx hasher in place of the default SipHash, which clippy rejects for two reasons. SipHash resists keys chosen to collide, which SPIT, reading the user's own files, has no need of, and it is slower on the small keys SPIT hashes most: product numbers, symbols, and bindings with their hash kept. And it is seeded at random, so a map's order changes from run to run: if that order ever leaked into output, the output would differ between runs, while with Fx the mistake is the same every run and a stored output catches it.
- **Output never depends on hash order.** Anything that reaches a file or the terminal is in a `BTreeMap`, a `BTreeSet` or a sorted `Vec`, or is sorted first. Given the same pipeline, inventory, root and version, the output is the same bytes every time (see `docs/architecture.md`).
- **Loops and worklists, not recursion, over data the user writes.** A pipeline may hold a chain of 100,000 steps, and recursing once per step overflows the stack (`compile/definitions.rs` has a test for this). Use an explicit stack or queue. Where recursion reads better, as in parsing nested types, cap the depth (`MAX_TYPE_DEPTH` in `src/types.rs`).

**Errors and invariants**

- **Errors are data.** An error is an enum variant whose fields say what went wrong, and `Located<E>` says where. Rendering is separate, in `src/diagnostics` and `render.rs`, so that the same error can be printed as text or as JSON.
- **Bad input never panics.** Outside tests, `unwrap` isn't used, and clippy rejects it. `expect` and `unreachable!` state an invariant the code already holds, and their message says what it is, as in `unreachable!("a product's template has its groups resolved")`. Anything a user's file can cause is an error.
- **Name the other half of an invariant.** When code relies on something another place guarantees, a comment says so with "Keep in step with" and names that place, as the `Ord` for `EntityBinding` does for its hash. Add the comment on both sides.
- **No `unsafe`.** The compiler rejects it.

**Files and modules**

- **A Rust file has at most 800 lines, tests included.** A longer file usually holds two subjects, which read better as two modules. Split it along the data it handles, as `src/paths` is split into `template.rs`, `rules.rs` and `bind.rs`, not into arbitrary halves. The files that were longer when the limit came in are listed in `OVER_LIMIT` in `tests/architecture.rs`, each with its length then. A listed file may shrink but not grow, and a file that falls within the limit leaves the list.

**Dependencies and boundaries**

- **Two crates, and a new one must earn its place.** SPIT depends on `mimalloc` and `rustc-hash`. `serde_json` was tried and removed, because the hand-written writer was 15% faster and needed five fewer crates (`1b4b141`). A new dependency needs `profiling/bench.py` numbers, and the output must stay byte-for-byte the same.
- **The three steps stay apart.** Compile, inputs and resolve don't reach into each other except as `tests/architecture.rs` allows. Every module is private, and the library's API is what `lib.rs` re-exports, so add to `lib.rs` only what a caller outside the crate needs.

## Checks before every commit

```sh
cargo fmt
cargo build --release
cargo test --release -q --no-fail-fast        # plain `cargo test` stops at the first failing test binary
cargo clippy --release --all-targets -q
usability/harness/rebuild_keys.sh             # expect `ok` for every answer key
```

- **Messie.** CI also checks the repository's folders with Messie:

  ```sh
  python3 -m venv /tmp/messie-venv
  /tmp/messie-venv/bin/pip install -q -r .github/messie-requirements.txt
  /tmp/messie-venv/bin/messie -af .   # -a judges every folder, -f checks each folder fits its surroundings
  ```

- **Stored outputs.** They are under `tests/fixtures/outputs/`. Re-save them with `SPIT_BLESS=1 cargo test --release --test outputs`, then read `git diff tests/fixtures` before committing.
- **Links.** A changed Markdown file's links and anchors should resolve. GitHub's anchor for `## \`dag --partial\`` is `#dag---partial`.
- **Speed.** For a change that claims a speed-up, give `profiling/bench.py`'s numbers before and after, and check that output is byte-for-byte the same.

## Conventions

- **Commit messages.** The title is a plain sentence, such as "Replace skip with drop, which names the groups it removes". Then prose saying what was wrong and what changed, then the co-author and session trailers.
- **No model names** in files pushed to the repository.
- **Guide updates travel with behaviour.** Each commit updates the README, `docs/language-reference.md`, `docs/spitdag.md` or `docs/architecture.md` for what it changes.
- **Verify before documenting.** Check every claim in the guide against the binary.
- **New recipe rule keywords** go in `split_rules` in `tests/support/mod.rs` as well, which separates a test's recipe rules from its pipeline by keyword.
