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

Some rules are checked, not just written down. `Cargo.toml` forbids `unsafe` and turns on clippy's `disallowed_types`, which `clippy.toml` sets to the standard library's `HashMap` and `HashSet` with their default hasher, and to `Rc`, `RefCell` and `Cell`. `src/lib.rs` and `src/main.rs` deny `unwrap` outside tests, in the library and in the command line. `tests/architecture.rs` checks file length and the boundaries between steps. `cargo clippy` and `cargo test` run these checks, so the checks before every commit cover them.

The five rules under [Performance](docs/architecture.md#performance) come first: one table with columns by id, text interned once, grouping by symbol keys, finding once and then looking up, and doing each piece of work once. The rules below are how the rest of the code keeps to them.

**Data and the functions over it**

- **Plain data, open fields.** A stage's input and output are structs and enums with public fields, such as `Pipeline`, `Job` and `ResolvedDag`. Add a method when it keeps an invariant that open fields can't, as `Artifacts` does with its columns. Don't add a getter or a builder for a field anyone may set.
- **Enums and `match`, not traits.** A closed set of cases is an enum, and code that handles them matches on it, so the compiler finds every place a new case touches. The crate has one trait, `Out` in `src/json.rs`, which lets the JSON writer write either to a string or to a hasher. Don't add a trait with one implementation, and don't use `dyn` outside the command line, `src/main.rs` and `src/cli`, unless it saves code.
- **Stages are functions.** A stage takes what it reads by reference and returns what it makes, as `resolve(&pipeline, &inventory)` does. It keeps no state between calls and holds no handle to an earlier stage.

**Ownership and identity**

- **One owner, and ids or borrows everywhere else.** A table owns its records. Anything else that needs a record holds its id (`ArtifactId`) or borrows it (`Artifact<'a>`). There is no `Rc`, `RefCell` or other shared mutable state, and clippy rejects them.
- **Ids are newtypes over `u32`.** An id wraps a `u32` so that ids of different tables can't be mixed up, and it turns into an index only at the table (`ArtifactId::index`). A job's `JobId` is its number from 1, not an index, since a DAG cut to one stage keeps its jobs' numbers. Convert a length into an id with `u32::try_from(..).expect(..)`, and give the limit in the message, as in "fewer than 2^32 artifacts in a DAG".
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

- **A Rust file has at most 800 lines, tests included.** A longer file usually holds two subjects, which read better as two modules. Split it along the data it handles, as `src/paths` is split into the template itself (`template.rs`), the path it gives one product's artifacts (`product.rs`), path components (`components.rs`), the checks on declared rules (`rules.rs`) and paths bound to a DAG (`bind.rs`), not into arbitrary halves. `tests/architecture.rs` fails a longer file.

**Dependencies and boundaries**

- **Two crates, and a new one must earn its place.** SPIT depends on `mimalloc` and `rustc-hash`. `serde_json` was tried and removed, because the hand-written writer was 15% faster and needed five fewer crates (`1b4b141`). A new dependency needs `profiling/bench.py` numbers, and the output must stay byte-for-byte the same.
- **The three steps stay apart.** Compile, inputs and resolve don't reach into each other except as `tests/architecture.rs` allows. Every module is private, and the library's API is what `lib.rs` re-exports, so add to `lib.rs` only what a caller outside the crate needs.

## The VS Code extension

[spit-vscode](https://github.com/eclnz/spit-vscode) is SPIT's editor extension, and it must stay in step with SPIT. It runs `spit check <file> --json --stdin --hovers` and shows the diagnostics, path hints and hovers that output holds. Its grammar, `syntaxes/spit.tmLanguage.json`, colours SPIT's keywords, headers and placeholders itself, and its README lists them.

- **A change to what the extension reads needs a change in the extension.** That is anything in `check --json` or `--hovers`, any keyword, header, placeholder or other syntax, and the file kinds `check` takes. Make the extension's change as part of the same work, with the same branch name in both repositories. The extension's `extension.test.js` and `grammar.test.js` should cover it.
- **Link the pull requests.** A spit-vscode pull request names the spit pull request it follows, as in "Follows eclnz/spit#42", and the spit pull request names the extension's. Merge the spit pull request first, since the extension needs a SPIT that has the change.
- **Versions move together.** The extension's version follows SPIT's, so a release of SPIT bumps the extension's `package.json` as well.

## The local runner

[spit-bash](https://github.com/eclnz/spit-bash) runs a `.spitdag`'s jobs on one machine. It reads only the DAG, so it must stay in step with `docs/spitdag.md`.

- **A change to the DAG format needs a change in the runner.** That is any field, argument part, ordering or meaning `docs/spitdag.md` gives, and above all a new `version`. Make the runner's change as part of the same work, with the same branch name in both repositories, and link the pull requests as for the extension. Merge the spit pull request first.
- **The runner's CI catches drift.** It builds SPIT from `dev`, where SPIT's work merges, and runs its examples on every push and weekly, so a format change merged without the runner's change shows up there as a failure.

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

  `ignore.messie` at the root leaves out the files Cargo and clippy need there under fixed names, which Messie would otherwise judge as unrelated to the rest of the root. Add a file there only when a tool requires its name and place.

- **Stored outputs.** They are under `tests/fixtures/outputs/`. Re-save them with `SPIT_BLESS=1 cargo test --release --test outputs`, then read `git diff tests/fixtures` before committing.
- **Links.** A changed Markdown file's links and anchors should resolve. GitHub's anchor for `## \`dag --partial\`` is `#dag---partial`.
- **Speed.** A change to performance-related code is benchmarked, whether or not it means to change speed. That is code that runs once per artifact, job, record or step, and any change to how data is laid out, copied, hashed, allocated or written: the model and its tables, parsing, resolving, binding, discovery, and the writers. Build the commit before in a worktree and run `profiling/bench.py pipeline` and `profiling/bench.py dataset` with `--old` set to it, as `profiling/README.md` shows, and check that output is byte-for-byte the same.
  - **It is a regression check.** With `--old`, each command compares the two builds at one size of its workload, takes a few seconds, and exits with status 1 when a time is more than 1.3 times the old build's and more than 5 ms slower. Give more sizes only to see how a stage scales, such as when a change claims a speed-up.
  - **Runs vary.** On a shared machine two runs of the same build can differ by 10% or more. When the check fails, run it again; a slowdown is a regression when it fails both times.
  - **No regressions.** Fix a regression before committing. If it is the price of something worth more, the commit message says what and why.
  - **The result goes in the commit message**: a line saying the check passed, or the times it flagged before and after. A change that claims a speed-up shows its `dag` and `check` columns before and after.

## Conventions

- **Branches.** Work merges into `dev`, which carries the next release's version. `main` holds the latest release, and moves to `dev` only when `dev`'s CI is green.
- **Issues.** Bugs, feature requests and work left for later go in the GitHub issue tracker, one issue each, not in a report's or a plan's list of next steps. A report or commit that leaves work undone links its issues.
- **Commit messages.** The title is a plain sentence, such as "Replace skip with drop, which names the groups it removes". Then prose saying what was wrong and what changed, then the co-author and session trailers.
- **No model names** in files pushed to the repository.
- **Guide updates travel with behaviour.** Each commit updates the README, `docs/language-reference.md`, `docs/spitdag.md` or `docs/architecture.md` for what it changes.
- **Verify before documenting.** Check every claim in the guide against the binary.
- **New recipe rule keywords** go in `split_rules` in `tests/support/mod.rs` as well, which separates a test's recipe rules from its pipeline by keyword.
