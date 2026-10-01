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
- Records of what happened, such as the usability study's round reports and their results archives under `usability/rounds/`.

**Links.** Permanent files (code, docs, tests and other notes) don't link to a plan, because the link breaks when the plan goes. Cite the commit that deleted the plan instead.
