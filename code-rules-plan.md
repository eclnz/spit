# Code rules plan

Bring the code in line with the rules under "Writing the code" in `AGENTS.md`, where it breaks them today. The files are split first, since the job changes add to `src/model.rs`, which may not grow. Output stays byte-for-byte the same throughout: every example's `check`, `dag` and `artifacts` output is compared with the build from before this work.

- [x] Small fixes: the two `sort_unstable_by` calls clippy flags in `src/paths/bind.rs`, `ArtifactInstance::key` copying its product's name, and `AGENTS.md` saying the standard `HashMap` is banned when the ban is on its default hasher. Commit 4a56f4c.
- [x] Split `src/model.rs` (2,082 lines) into `src/model/`. Commit 799a84a.
- [x] Split `src/diagnostics.rs` (1,681 lines) into `src/diagnostics/`. Commit c5ae0aa.
- [x] Split `src/main.rs` (1,252 lines) into `src/cli/`. Commit COMMIT.
- [ ] Split `src/paths/template.rs` (842 lines).
- [ ] Split `src/parser/inventory.rs` (804 lines).
- [ ] Empty `OVER_LIMIT` in `tests/architecture.rs`.
- [ ] Job ids: a `JobId` newtype for `Job::id`, `Job::dependencies`, `BoundJob::id` and `BoundJob::depends_on`, in place of a bare `usize`.
- [ ] A step table: `ResolvedDag` keeps each step's operation and stage once, and a `Job` holds its step's id instead of copies of both. `BoundDag` does the same, with each step's port names, so a `BoundJob`'s ports are artifact ids in port order. Binding finds each step's commands once instead of searching every command for every job.

Delete this plan in a separate final commit before the PR merges.
