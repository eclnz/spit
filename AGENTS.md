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
