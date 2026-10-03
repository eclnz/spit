# SPIT design review: graph, inputs, checks, and execution

Conversation notes, 3 October 2026. Claims about the current state refer to the usability branch of eclnz/spit (now `dev`) and the separate spit-bash runner as they were reviewed during that conversation. Proposed features are design ideas, not claims about implemented syntax.

This is a plan in the sense of `AGENTS.md`: it lives on `dev` while its work goes on, and the last commit of that work deletes it. Several agents work from it at once, so the first section says how.

## Working from this plan

Work is trunk-based. `dev` is the trunk, and every agent reads this file from it.

- **Claim before you start.** Set the item's status in the tracker below to `claimed` with your branch name, commit that change alone, and push it to `dev`. If the push is rejected because someone else's claim landed first, pull, and pick another item if yours is taken. A claim keeps two agents from building the same thing; it does not lock the code.
- **Small branches, merged soon.** Work on a short-lived branch named after the item, keep it close to `dev` by merging `dev` in often, and merge back as soon as the checks in `AGENTS.md` pass. Prefer several small merges to one large one: other agents are changing the same tree.
- **Design questions first.** Many items below are questions, not decisions. An item that changes syntax, the DAG format or the runner contract starts by writing its answers to the four questions in [Design principle for agents](#design-principle-for-agents-lower-cleanly-or-draw-a-new-boundary) into this file, under the item, and merging that before the code. Then other agents see the decision before they build on it.
- **Record what landed.** When a step merges, mark it `done` with the commit that did it. Leave a note under the item when something was decided or deferred, so the next agent does not reopen it.
- **Keep the rules.** Everything in `AGENTS.md` still holds: the checks before every commit, the same branch name in spit-vscode or spit-bash when a change reaches them, and the guide updated with the behaviour.
- **When the plan is done.** The last item's branch deletes this file and the temporary pointer to it at the top of `AGENTS.md`, in one commit that does nothing else, as `AGENTS.md` describes. Items still worth doing then go to GitHub issues.

## Tracker

Status is `open`, `claimed (<branch>)`, `decided`, or `done (<commit>)`. Numbers refer to the sections below.

| # | Item | Status |
|---|------|--------|
| 1 | Optional runtime checks in the job contract | done (`df6a61a`) |
| 2 | Optional-file and output semantics, from one real workflow ([#35](https://github.com/eclnz/spit/issues/35)) | open |
| 3 | Lower the cost of binding existing data: judge `inputs --suggest` on irregular datasets | done (`7f12083`) |
| 4a | `dag --counts` | done (`370a18f`) |
| 4b | Diagnostic severity audit ([#45](https://github.com/eclnz/spit/issues/45)) | done (`b45f28e`) |
| 5 | The two-file mental model | done (`b72f2e9`) |
| S1 | Syntax: operations declared inside a stage | done (`ff3ec35`) |
| S2 | Syntax: `@ min(n)` beside the `many` port | done (`5131495`) |
| S3 | Syntax: `require` and `drop` clause order ([#36](https://github.com/eclnz/spit/issues/36)) | done (`3c6d843`) |
| S4 | Syntax: shell metacharacters in commands | decided, claimed (`shell-meta`) |
| S5 | Syntax: `path:` in both `.spit` and `.spitin` | done (`b72f2e9`) |
| R1 | Composite operations: provenance and diagnostics design | open |
| T1 | Sidecar `incomplete_groups` keyed by structured bindings ([#37](https://github.com/eclnz/spit/issues/37)) | done (`5ab3a16`) |
| T2 | Incomplete-group warning for `InputSource::Inventory` | done (`8ef0e50`) |

Deferred, and not to be claimed without a concrete use case: resources, Slurm, one execution package, dynamic outputs. See [Future direction](#future-direction-deliberately-deferred).

## The product goal

A user should be able to describe a processing workflow in concise text, bind it to a real dataset, and inspect a predictable, valid graph of concrete jobs before running it. SPIT owns the meaning and resolution of that graph. A runner executes it. The JSON `.spitdag` is the interface between them.

The practical usability test is a first clean run: can someone write a short pipeline, point it at a folder, see the discovered and missing inputs and the resulting jobs, correct mistakes, and hand a plan to a runner? If the author knows what they want but cannot express or inspect it without guesswork, that is a higher-priority gap than syntax polish.

"Valid graph" has a precise limit: SPIT can check declarations, types when supplied, dimensions, cardinality, source binding, paths, and dependencies. It cannot infer that a file called an MRI is truly an MRI with the expected contents. The execution layer can apply additional checks declared by the workflow.

## What is working well today

- **Text-first workflow definition.** Users write `.spit`, not Rust. Products, operations, selectors, commands, and stages make a statically resolvable DAG; types are optional and can add stronger checks without making a small pipeline verbose.
- **Separation of pipeline and dataset.** The pipeline can be checked without a dataset. `spit inputs` discovers or settles source artifacts; `spit dag` binds those inputs into concrete jobs and emits structured JSON. A simple pipeline can scan a folder directly with `--root`; a `.spitin` recipe is available for dataset-specific paths, discovery, exclusions, drops, and requirements.
- **Early, specific feedback.** The compiler supplies diagnostics and hover data to the VS Code extension. Source path misses can name the nearest file and show the mismatch. `artifacts` explains incomplete results and unused sources. The extension presents compiler knowledge rather than independently implementing language semantics.
- **Evidence-driven usability work.** The project has already run several usability rounds and changed the language and paths in response; this is not merely a proposed next step. See [PR 31](https://github.com/eclnz/spit/pull/31).
- **A real execution boundary.** spit-bash already reads a saved DAG or runs `spit dag` from a recipe. It executes existing `verify` commands before the job command, requires declared outputs to exist, tracks successful work, skips current jobs, and blocks dependents after a failure. The current language reference and DAG format define the pre-command `verify` behaviour. File existence is the current default success check; a domain-specific check such as "this NIfTI image is 3D" is not yet a post-command contract.

## Where the boundaries belong

| Concern | Natural owner | Reason |
|---|---|---|
| Products, operations, type and dimension relationships, selectors, commands | `.spit` and the SPIT compiler | These define the reusable graph and the work a job performs. |
| Dataset root, source path bindings, exclusions, coverage rules, unusual subjects | `.spitin` when needed | These change how one dataset binds to the same graph. A trivial dataset need not require a recipe. |
| Concrete inputs, outputs, commands, checks, dependencies, and optional resource requests | `.spitdag` JSON | Every backend receives the same resolved job contract. |
| Process execution, checking files at runtime, reruns, logging, local concurrency | spit-bash | These depend on actual files and process outcomes. |
| Cluster account, partition, queue policy, submission and monitoring | A future Slurm adapter and its site configuration | These vary by computing environment, not by scientific workflow. |

The overlap between `.spit` and `.spitin` should be deliberate placement and fallback, not interchangeable files. A reusable source path can live in `.spit`; a path specific to a dataset can live in its recipe. Under the current rules, the same source cannot receive its own path rule in both places; a recipe default can cover a source left without one. Operation definitions and steps stay in `.spit`. Any proposed override, such as a dataset-specific resource exception, needs explicit precedence. Making both files fully identical would obscure which graph actually ran; making them wholly separate would add ceremony to the simple folder workflow.

## Next design work, in priority order

### 1. Make optional runtime checks part of the job contract

Keep declared-output existence as the universal baseline. Do not require nonzero file size universally: an empty text file can be a valid result, and an output folder may validly be empty. Let a workflow opt into stronger checks.

The authoring model should avoid one declaration per subject or artifact instance:

1. Define a reusable, parameterised check once, for example an external command that fails unless an image has a given number of dimensions.
2. Attach it by default to an operation's input or output port, so every call inherits it. Allow a product-specific requirement for a source with no producing operation or for an exceptional result.
3. Have SPIT validate and bind the check into the DAG for each concrete job or source. The runner runs input checks before consumption and output checks after production, before marking success or releasing dependents.
4. Report failure against the concrete artifact and the named requirement. A failed check is a failed job, even when the processing command exited zero.

This is separate from static types: `Image<Subject>` expresses coordinate-space intent, while "3D" needs file inspection. SPIT should not grow a built-in parser for every domain's file format; a small command using `mrinfo` or another domain tool can provide the test.

Rerun rule: a changed or newly added check must invalidate a previous success claim. Decide whether the runner rechecks an existing output without recomputing it or reruns the job. The DAG fingerprint and runner state must not silently treat an unchecked output as validated. The existing `verify` is pre-command and cannot directly express an output postcondition.

This changes the DAG format, so it needs the matching change in spit-bash (see `AGENTS.md`), and syntax that the extension's grammar must learn.

**Item 1 design (decided, branch `runtime-checks` in all three repositories).** A `check` declares a test of one artifact once; `@ check(...)` attaches it to a port, beside S2's `@ min(n)`, or to a source:

```text
check ndim(n): check_ndim {@path} {n}
check nonempty: test -s {@path}

source t1w : Image .nii.gz [sub] @ check(ndim(3))
operation denoise(dwi: DWI @ check(ndim(4))) -> DWI .mif @ check(ndim(4), nonempty)
operation split(items: many Table @ min(2) @ check(nonempty)) -> (left: Table @ check(nonempty), right: Table)
```

- *Declaration.* `check name: command` or `check name(param, ...): command`. The command is an ordinary command template. `{@path}` is the artifact being checked, and each `{param}` is the literal text given where the check is attached. It must use `{@path}`, and may use nothing else: no port, no output, no `.dir` or `.stem`. A check reads one artifact and never adds a dependency. Checks are global, as operations are, and `use` imports them with the operations and sources that attach them.
- *Attachment.* `@ check(a, b(1), ...)` follows an input port's type, an output's type and extension, or a source's dimensions. Arguments are bare words or double-quoted text, and their count must match the parameters. A `many` port checks each artifact in its collection. Several checks accumulate, and none overrides another. Checks on a step's result product, and checks in a `.spitin`, wait for a concrete case.
- *Where they run.* An input check, from an input port or from the source that the artifact belongs to, runs before the job's `verify` and command. An output check runs after the command and after the existence check, before the job counts as a success. A failed check fails the job, even when the command exited 0, and the report names the check as written (`ndim(4)`), the port and the artifact's path. When the producing job in the same DAG already runs the identical command as an output check, the consumer's input check on that artifact is left out. A source's checks are input checks of each job that reads it.
- *Rerun rule (question 3).* Recheck existing files; don't recompute. Checks are left out of the job `fingerprint`, which stays the identity of the job's work, so a changed check doesn't rerun a long job. A runner instead records which checks passed with each success, and when a current job's checks differ from its record, it runs the checks alone on the existing files. If one fails, the job fails and its record is dropped, so the next run recomputes the job. `adopt` records no checks, so the next `run` checks adopted outputs without recomputing them.

The four questions:

1. *Phase.* The parser owns `check` lines and `@ check(...)` clauses. Compile checks names, arity and each check's template, just as it checks commands at load. The resolver binds checks to concrete artifacts beside `verify` (`resolver/bind.rs`), and `src/spitdag` writes them.
2. *Lowering.* Checks get a model of their own, `CheckDef` (a name, parameters and a `CommandTemplate`), plus a list of uses on `InputPort`, `OutputPort` and `ProductDef`. `verify` checks a job across several inputs before it runs, so it can't express an output postcondition or a single artifact's check. Checks reuse the command template, its quoting and its argument parts, and the `executables` list. Bound, each check is one entry in a new job field, `checks`, in DAG version 6. Each entry is `{"when": "before"|"after", "check": "ndim(4)", "port": "dwi", "path": P, "command": [ARGUMENT, ...]}`, ordered with the before checks first, then by port, then by artifact, then in the order the checks are attached.
3. *Invariants.* Checks never change the graph: jobs, paths, dependencies, counts and fingerprints are the same with or without them. The order is deterministic. A check reads only its artifact. Names are unique among checks, and a check name may match an operation or product name, as their namespaces are separate.
4. *Interactions.* Selectors: none, since checks attach to ports, not to calls. Imports: as above, prefixed as operations are. Stages: none. Paths: `{@path}` is the bound path, a folder's included. Partial plans: only planned jobs carry checks. Diagnostics point at the clause or the declaration. A port's hover lists its checks. `dag --commands` shows `check:` lines. `executables` includes the checks' programs. spit-vscode's grammar learns `check`, `@ check(...)` and `{@path}`. spit-bash reads version 6, runs the checks, and adds a `check` plan status.

*Done in `df6a61a`* (spit, from `18172c7` and the merge with S2 in `33a2bf7`), with spit-bash `926333a` and spit-vscode `6d69212`, each merged into its `dev`. As built, and where it differs from the design above:

- *Arguments are bare words.* Quoted text is left out until a check needs a space or a comma. An argument may not hold whitespace, quotes, braces, commas, parentheses or `@`.
- *Errors.* A check must use every parameter it declares. A check with no parameters is written without parentheses, both where it is declared and where it is used. `@ check(...)` after an operation's outputs, or on a step's product, is an error that says where the check goes.
- *Imports.* `use ndim from checks.spit` imports a check by name, and a whole-file `use` brings every check. Two imports of one identical check are one check.
- *Rerun state.* spit-bash keeps a hash of each job's `checks` in its record, in state version 3, and still reads version 2. A job with no command whose outputs exist is checked on every run. A dependent that is skipped as current is not held back by a failed recheck upstream; the failed job's record is dropped, so the next run reruns it and its dependents.
- *Deferred:* checks on a step's result product, checks in a `.spitin`, and hovers on a check's name where it is used (the declaration and the clause word have hovers).

### 2. Resolve optional-file and output semantics with a real workflow

[Issue 35](https://github.com/eclnz/spit/issues/35) leaves open optional sidecar members and outputs a command may or may not create, as well as the backend's missing-output contract. Work through one genuine case before adding a general optional type. If a sidecar is absent, the graph needs to say whether to skip the job, use a different operation, proceed without that argument, or fail. "Optional" syntax alone does not settle that behaviour.

### 3. Lower the cost of binding existing data

The project has improved path diagnostics and can scan a dataset without a recipe. [PR 52](https://github.com/eclnz/spit/pull/52), open when reviewed and since merged, adds `spit inputs --suggest` to generate source and path lines from existing files. This addresses a usability-study finding directly. Judge it by whether a user can inspect, correct, and trust suggestions for irregular datasets; keep suggested declarations editable and verify them with the actual matcher.

*Done in `7f12083` (merged in `9ec8bcb`; the module split before it is `2b0412b`).* Judged on five irregular trees: BIDS with a rescan (`acq-rescan`), a missing session, a session-less subject, `derivatives/` and `sourcedata/`; a lab's own layout with `subject04/visit1` among `Subject01/Visit1`, a `.bak`, a `_repeat` scan and a name with a space; loggers under `site_north/2023/` with `_corrected`, `.CSV` and `~` files; a plate of wells; ML runs by config and seed. Every pasted rule read its files, so the rules could be trusted, but a suggestion was hard to inspect and easy to spoil. What changed:

- *Inspect.* Each group prints the values each dimension holds, so `Notes, notes` or a decimal split at its `.` shows before pasting. A file a suggested rule nearly matches is listed with that rule and where the two part, in the words a missed source's nearest file uses, instead of only "like no other".
- *Correct.* A stray folder no longer spoils its group: when at least three files in four share their keys and the rule then names more dimensions, the others are left out. Dimensions are named after a `word_` before them, or `date` and `year` by their values, before `dimN`. `site_north/` and `site_south/` are one folder. A second group of one suffix is named for its own word (`bold_nback`, `bold_preproc`), and no source takes a dimension's name.
- *Trust.* A rule whose file name is only dimensions (`{dim1}` for `README` and `CHANGES`) is no longer suggested, since it read every file and folder beside them.
- *Left as they are.* A value cannot hold `.`, so `lr-0.01` gives `lr-{lr}.{dim1}`; the values line shows it, and the fix belongs to the value rules, not to suggestions. Case differences are not folded (`subject04` is listed alone, not near `Subject{subject}`), and a plain top folder such as `baseline/` beside `lr-0.01/` still keeps its files apart. Suggestions ignore the pipeline's existing rules when finding near misses; `inputs` already names a missed source's nearest file. None of this reaches `check --json`, the DAG or spit-bash.

### 4. Make a resolved graph easy to sanity-check

A proposed `dag --counts` view would show total jobs and counts by operation. That would quickly reveal unintended expansion or an unexpectedly empty stage. It improves inspection rather than expressivity, so it follows the job-contract questions in priority.

*4a done in `370a18f`.* `dag --counts` prints one row per step (`cleaned = clean`, its stage, its job count), keeps steps with no jobs as `0`, then the total; it combines with `-o`. It counts the jobs planned: with `--partial`, the artifacts left out are not yet counted per step, which `IncompleteJob` would need a step id for.

Keep the severity of compiler feedback consistent with whether it blocks a valid graph. [Issue 45](https://github.com/eclnz/spit/issues/45) asks whether errors that do not fail `check` should instead be warnings, especially in the VS Code extension. Audit those cases at the compiler's diagnostic level so CLI and editor users see the same distinction; the editor should render severity rather than decide it.

*4b done in `b45f28e`.* The audit found no diagnostic that the editor marks as an error while `check` passes. Issue 45's example, the note about `--strict-paths`, went with that flag in `b1c5dbd`. Severity is set once, in `src/diagnostics`. `check` prints that list and fails exactly when it holds an error. `check --json` gives the editor the same list, and spit-vscode maps `warning` to a warning and anything else to an error, so it decides nothing. Each later command runs the diagnosis of the steps before it, so an error stops them all. The warnings are all right as warnings. They are unused or misnamed definitions, an empty stage, an unbound output type variable, a shell operator, a `#` joined to a word, and an operation with no command, which still makes a valid graph since `command` may be `null`. With inputs there are also empty steps, case collisions, dashed labels and near misses. One warning misstated what follows it. A recipe's missing `root` said `inputs` would "find no files", but `inputs` stops with an error. It stays a warning, since the folder may exist on the machine that runs the recipe, and the message now says that `inputs` and `dag` stop. `tests/severity.rs` holds `check` and `check --json` to the same diagnostics and severities, for each warning and for the examples. `docs/architecture.md` states the rule. spit-vscode needs no change.

*Deferred, not a severity question.* `check` on a recipe that writes its records inline settles them, but it does not report what `dag` then reports for them: the incomplete `sidecars` group warning (T2), and a job input those records lack. `check` reads no data, and resolving jobs belongs to `dag`. Showing the group warning in the editor would need `incomplete_groups` to return structured groups that can be placed on a record line, not strings. It is worth an issue if inline records are used much.

### 5. Clarify the two-file mental model

Explain `.spit` in one sentence as the reusable graph and `.spitin` as a dataset binding and exception layer. Review each shared directive for clear default and override semantics. Keep the no-recipe `--root` path for simple cases rather than requiring a nearly empty recipe. Use real first-pipeline attempts to find where users cannot tell which file owns a declaration.

**5 and S5 design (decided, on `two-files`).** Handled together: `path` is the only directive both files take, so the two-file model and S5 are one question. No syntax changes; the work is the explanation and the diagnostics.

- *Evidence.* Across rounds 1 to 4, one participant (round 1, s1-logs-b) wrote a source's path in both files, and understood `` `log` has path rules in both .spit and .spitin `` at once, asking only for a line number and for the guide to say which file to prefer (round 1 findings D7, B3). Round 3's cohort participants put source paths in the recipe in one run and in the pipeline in the other, with no hesitation either way. The commonest doubt, about 8 participants in round 1 (D1), was whether a recipe needs any rule at all; the no-recipe `--root` path since answers it, and stays. Nothing in the rounds asks for a new directive or a different precedence.
- *The model in one sentence each.* A `.spit` pipeline is the reusable graph: what work to do and where its results go, for any dataset. A `.spitin` recipe binds that pipeline to one dataset: where its folder is, where its sources are when the pipeline does not say, and which of its data to leave out or require.
- *Ownership.* Every directive belongs to one file, except `path`. Pipeline only: `source`, `sidecars`, `dimensions`, `operation`, `command`, `verify`, steps, `stage`, `use`, `ext:`, and an output's `path`. Recipe only: `pipeline`, `root`, `discover`, `exclude`, `drop`, `require`, records. A source's own `path name:` goes in either file, never both. A default `path:` goes in either, with different reach: the pipeline's covers outputs and any source no other rule covers, the recipe's covers sources only.
- *Precedence, kept as it is.* A source takes its own rule (from whichever file has it), else the recipe's default, else the pipeline's default. The dataset's word on where its inputs are beats the pipeline's general default; a rule written for one source beats any default. Outputs never read the recipe. This is already implemented and documented; a test now pins all four levels.
- *Diagnostics.* A declaration in the wrong file says which file owns it, at its line: a step in a recipe, `root` in a pipeline, an output's or unknown product's `path` in a recipe, and a source path written in both files, which now points at the recipe's line and says how to choose. Message text and places only; the `check --json` shape is unchanged, so spit-vscode needs no change.
- *Docs.* The README and the language reference open their recipe sections with the two sentences and the ownership table, and give the precedence as a list.

*5 and S5 done in `b72f2e9`.* As designed, plus a new `InputError::OutputPath` for an output's `path` in a recipe, and the README's smallest recipe gaining the `root` line a recipe requires. `tests/two_files.rs` pins the precedence and each message at its line. Deferred: the both-files error does not give the pipeline's line, since the recipe's diagnosis holds only the compiled `Pipeline`, not its source map; the message names the file, and nobody in the rounds asked for more. If round 5 shows people still unsure which file owns a declaration, reopen this with that evidence rather than adding a directive.

## Language syntax audit

The central idiom is promising: `source` declares artifact families, `operation` declares transformations, and `result = operation(inputs)` builds the flow. Optional types and selectors on call arguments support that idiom. The items below are candidates to test, not accepted language changes. Compare them against real pipelines, the usability-study findings, diagnostics, and migration cost before changing the grammar.

| # | Current syntax or behaviour | Why it warrants review | Candidate direction |
|---|---|---|---|
| S1 | An `operation` and its `command` may be declared inside `stage preprocess:`, but the operation remains usable everywhere. | Indentation appears to define lexical scope while the compiler treats the definition as global. | Either keep reusable definitions at the top level or in imported libraries and reserve stages for steps and stage defaults, **or** give stage declarations real scope with an explicit export. Test both against the large MRtrix example before choosing. |
| S2 | `operation summarise(days: many Series) -> Summary @ min(2)` | `@ min(2)` constrains the collected `days` input but sits after the output type. That becomes misleading if the language ever permits more than one `many` input. | Put the minimum beside the `many` port it constrains. Keep plain `many` with no minimum. Check how diagnostics, hovers, and existing examples would change. |
| S3 | `require t1w count=1 per [sub, ses]` and `drop [sub, ses] where t1w count<2` | Related group rules use different clause order ([issue 36](https://github.com/eclnz/spit/issues/36)). | Test a shared group-first form such as `require [sub, ses] where t1w count=1`, keeping the distinction that `require` fails and `drop` removes. Do not change it for visual symmetry alone if study participants find the current form clearer. |
| S4 | `command process: ...` uses familiar shell quoting, but unquoted `\|`, `>` or `&&` are literal arguments; SPIT currently warns. | Readers may expect shell behaviour and get a runtime error that could have been caught during checking. | Not settled in the review. Decide whether these become errors, an explicit shell form, or stay warnings with a clearer message; make the notation reflect the execution model either way. |
| S5 | `path:` and `path product:` are allowed in both `.spit` and `.spitin`, with ownership and fallback that depend on context. | The same words can obscure whether a rule is reusable or dataset-specific. | Explain and test the existing precedence as an intentional default and fallback; improve diagnostics before adding further overlapping directives. A source cannot currently have its own path rule in both files. |

**S2 design (decided).** Write the minimum on the `many` port it constrains, with the same `@ clause(...)` form a call uses on the argument it selects (`input @ vary(run)`):

```text
operation summarise(days: many Series @ min(2)) -> Summary
operation fit(waves: many Table @ min(2), policy: Policy) -> Coef
operation collect(items: many @ min(3)) -> Bundle
```

- *Evidence.* Study participants in rounds 1 to 3 read the trailing `@ min(2)` correctly every time ("needs at least two weekly revenue artifacts"), so this is not a fix for observed confusion. It is for consistency: item 1 attaches checks to ports, and that syntax should find port modifiers already beside their ports. Plain `many` with no minimum is unchanged.
- *Rejected.* `many(2) Series` reads as exactly two; `at least 2 Series` adds words; keeping both forms gives two ways to write one thing.
- *Migration.* The trailing form becomes an error that gives the rewrite for that line, as the removed `@ drop(...)` does. SPIT is pre-1.0 and has done this before (`{output}` to `{@output}`).
- *Phase and lowering.* The parser owns it. It lowers to the existing `OperationDef::minimum_collection`, so compile, resolve, `--partial`, imports and the resolver's messages are unchanged. An operation takes at most one `many` input, so the operation-level field is exact; if that limit is ever lifted, the field moves to `InputPort` then.
- *Invariants and diagnostics.* A positive integer, once per port, only on a `many` port; each error points at the clause on its port. The hover shows the minimum beside its port.
- *Beyond spit.* spit-vscode's grammar marks `@ min(n)` inside the parentheses, on a branch named `many-min`; the s6 harness scenario's pipeline is rewritten (its answer key's jobs do not change). The DAG format and spit-bash are untouched.

*Done in `5131495`, with spit-vscode `a2be334`.* As designed. Also: `rebuild_keys.sh` stopped silently when a key failed to resolve, which hid the s6 break until its exit status was checked; it now prints `FAIL` with the error. Agents running it should still check its exit status.

**S3 design (decided: keep both orders).** `require t1w count=1 per [sub, ses]` and `drop [sub, ses] where t1w count=0` keep their shapes. The grammar does not change; the only change is diagnostics: a rule written in the other rule's order is an error that gives the line in its own order.

- *Evidence.* In `usability/rounds.zip`, every `drop` written in rounds 2-pilot to 4 (13 runs, all `drop [sub] where sessions count<2`) was written right first time, and reports singled out its note (`dropped [sub=03] by drop [sub] where sessions count<2`) as the most useful message. `require` was written once, in round 1 (s2-cohort-a), correctly and unprompted: `require t1w count=1 per [sub, ses]`. No run misread or miswrote it, so there is no confusion for alignment to fix. The removing-inputs design (`git show 1b30e26^:usability/design/removing-inputs.md`) left alignment open for the same reason: `require` "already reads correctly".
- *Why not align.* Round 1's B1 is the evidence on shared shapes: `skip` shared `require`'s grammar with the opposite action, and `skip bold run=3 per [sub, ses]` kept the very sessions it meant to remove. `drop`'s order is part of that fix: its `where` filters, naming the groups that go. In `require [sub, ses] where t1w count=1` the same `where` would have to assert, the opposite polarity behind one word, and it reads as a filter ("require the sessions where t1w count=1"), so two rules that differ by one keyword would mean opposite things. `drop`'s `missing` and `has` have no `require` counterpart either. That is change for visual symmetry alone, which the audit rules out.
- *Rejected.* `require [sub, ses] where …` (above); `drop t1w count=0 per [sub, ses]`, which undoes the B1 fix; accepting both orders, which is two ways to write one thing.
- *What changes.* Writing `require [sub, ses] where t1w count=1` or `drop t1w count=0 per [sub, ses]` by analogy with the other rule gives an error naming the rule in its own order, as `skip` does now. `require … where … missing …` and a `drop` with values but no `has` or `missing` get the general shape, since their intent can't be read off.
- *Phase and lowering.* The recipe parser only; `CoverageRule`, settling, the `.spitout`, the DAG and spit-bash are untouched. spit-vscode's grammar colours both keywords already and needs no change.

*Done in `3c6d843`* (merged in `79f87fe`). The errors give the rule in its own order, and the language reference says why the two orders differ. Issue #36's bullet on one grammar for `require` and `drop` is answered by this decision; its other two ideas stay open there.

**S4 design (decided, branch `shell-meta`).** A command stays a list of program arguments, which is how every backend runs it, and an unquoted shell operator becomes an error. A command that needs a pipe or a redirection names its shell, as any other program, and takes its paths as positional parameters:

```text
command first: sh -c 'cut -f1 "$1" > "$2"' sh {table} {@output}
command bad:   cut -f1 {table} > {@output}
#   error: `>` in the command for `bad` is not a redirection: commands run without a shell.
#          Quote it ('>') to pass it to the program, or run a shell: sh -c '... > "$2"' sh {table} {@output}
```

- *Evidence.* No participant in rounds 1 to 4 wrote a shell operator in a command, so this is about a mistake the checker can see, not one the study saw. Checked against the binary on `dev`: the `sh -c` line above works today, and `dag --commands` prints it as `sh -c 'cut -f1 "$1" > "$2"' sh x.txt out/col/name=x`. The bare `>` becomes the argument `'>'`, and the job fails only when it runs (`cut: '>': No such file`), or `dag -o` saves it with no more than a warning.
- *Why an error.* A bare `|`, `>`, `&&`, `;` or `2>&1` never does what it looks like, and quoting it keeps the literal meaning with one character more, so the error takes away nothing anyone can mean. It is the same kind of rule as the old `{output}` spelling: likely-mistaken text with one fix.
- *Rejected: an explicit shell form* (such as `shell first: cut -f1 {table} > {@output}`). Every placeholder would have to be quoted for the shell, and correctly in each context (bare, in `"..."`, in `'...'`, inside `$(...)`), so SPIT would need a shell quoting model it does not have. It adds a keyword the extension's grammar and the guide must learn, and the program it runs is still `sh -c`, which the explicit idiom already states. Positional parameters (`"$1"`) mean a path is never parsed again as shell text, so spaces and quotes in paths stay safe. Revisit only if real pipelines show the idiom is too noisy.
- *Rejected: a clearer warning.* A warning doesn't stop `dag -o`, and the job fails later on the runner, which is the cost the review names.
- *Phase and lowering.* The parser owns it: `CommandTemplate::parse` already marks each bare operator word while it splits the template, so the operator is reported as a `CommandProblem` there, located on its column, instead of being stored for a later warning. Nothing new is lowered. The DAG format, spit-bash and `--commands` are unchanged.
- *Invariants and diagnostics.* Every argument in the DAG means what it looks like. Quoted or escaped operators (`'>'`, `\;`) stay literal arguments, and operators inside a quoted `sh -c` script are not looked at. The rule applies to `verify` commands as well, and to an imported library, whose error is reported in the library, as any other command error is. The warning and its "unquoted shell operator" line in the README go; `check --json` gives an error where it gave a warning.
- *Beyond spit.* The extension shows the severity that `check --json` gives, and its grammar doesn't colour operators, so no change is expected there; that still needs checking against `extension.test.js` on a branch named `shell-meta`. The language reference documents the `sh -c` idiom. Item 4b should count this as one warning moved to an error.
- *Considered, left out.* An unquoted `$VAR`, a backtick or a glob such as `*` is also passed literally, but each one is a legitimate argument to some programs, which isn't true of an operator word. They stay as the guide describes them.

**S1 design (decided: definitions stay global, and their place must hold their calls).** Neither candidate as written. An `operation`, its `command` and its `verify` may still be declared inside a stage, and stay global, so imports, the DAG and every later phase are unchanged. What changes is that a stage-declared operation called outside the stage that declares it is a warning at the declaration, naming the call and where to move the declaration: the innermost stage that holds every call, else the top level. Indentation then tells the truth about where an operation is used, without making stages into modules. A duplicate operation name, which is where reading a stage as a scope goes wrong today, says the operation is global and where the first one is.

- *Test against the large MRtrix example* (`examples/commands/mrtrix3_act`). All 24 operations are declared in the stage that uses them, and 3 are called from a sibling stage: `extract_b0` and `mean_b0` (declared in `preprocess/combine`, called again in `preprocess/correct`) and `mrtransform` (declared in `anatomy/registration`, called again in `anatomy/tissue`). Each candidate was written out and compiled:
  - *Top level or library only* (stages reject `operation`, `command`, `verify`): the 24 definitions move to an imported `mrtrix.spit` (85 lines) and the pipeline drops from 166 to 108 lines, with a byte-identical `.spitdag`. The stages read as a flow, but every step's command and its comment ("SynthSeg writes NIfTI, so...", "-rpe_header uses the phase-encoding metadata...") is now in another file from the step it explains, and the tool library is specific to this one pipeline, so it is reuse in name only.
  - *Real scope with an explicit export*: 3 `export` markers in this file, and no other change. But for that, the compiler gains scoped name lookup per stage, an `export` keyword the extension's grammar must learn, a rule for what `use` brings from an imported file's stages, and a choice between keeping names globally unique (scope then only adds errors, no expressive power) or allowing two stages a `denoise` each, which needs qualified names in the `.spitdag`'s `operation` field and so a change to spit-bash. That is the module machinery the [reuse section](#reuse-without-hiding-the-graph) says a stage need not have.
  - *Chosen*: the 3 shared operations move up one level, `extract_b0` and `mean_b0` to `stage preprocess:` and `mrtransform` to `stage anatomy:`, a 4-line diff per move with a byte-identical `.spitdag`. The other 21 stay beside their steps. `extract_b0`'s own comment already says it is reused after correction, so its new place says what the comment says.
- *Study evidence.* In `usability/rounds.zip`, 49 participant pipelines declare operations. Of the 5 with stages, 4 (all s5-survey, rounds 1 and 2) declared each operation inside the stage that calls it, unprompted, and 1 (s2-cohort-r4) declared all at the top level. None called a stage-declared operation from another stage, and no report misread where an operation could be used. Declaring beside the step is what participants do when nothing stops them, so removing it would cost the happy path for a confusion nobody showed. The warning only fires where the global meaning is visible: a call outside the declaring stage.
- *Rejected.* Top level only, for the reasons above and because it breaks 4 of the 5 staged study pipelines and the stages example for no observed confusion. Real scope, above. Silence (keep the docs sentence alone): the docs already say "Operations and commands stay global", and the MRtrix example still drifted into sibling-stage reuse.
- *Phase and lowering.* The parser records the stage a declaration sits in, as it already does for a step, and lowering keeps it in `SourceMap` beside the declaration's place; the model, `OperationDef`, compile, resolve, the `.spitdag` and spit-bash are untouched. The check is a lint in `src/diagnostics/warnings.rs`, beside "stage has no steps", using `stage_within`. It reads each local step once, with one lookup per step.
- *Invariants and diagnostics.* No change to what checks or resolves, so the `.spitdag` stays byte-identical for every pipeline. One warning per operation, at its declaration, in line order; it names the first outside call's line and stage and the stage to move to. Imported operations get none: `use` is the explicit way to share, and a library is linted on its own. Commands and `verify` follow their operation by name and are not checked separately.
- *Beyond spit.* No syntax changes, so spit-vscode's grammar is unchanged; the warning reaches the editor through `check --json` like every other located warning, and `extension.test.js` needs no change. The MRtrix example moves its 3 shared operations. The one harness scenario that declares operations in stages, s5-survey, calls each only in its own stage, so it gets no warning and no answer key changes.

*Done in `ff3ec35`.* As designed. The duplicate-name error moved from compile into lowering, as a duplicate stage's did, so only the first duplicate in a file is reported, and it names the line of the first declaration, or the `use` that imports one. Left out on purpose: checking where a `command` or `verify` sits against its operation's stage, and an editor hover naming an operation's stage; neither had a case behind it.

The proposed language design rule: place modifiers beside the construct they constrain; make indentation's scope obvious; order related clauses consistently; make the command notation reflect its actual execution model. S1 and S2 deserve attention before adding check and resource syntax, since those features would otherwise inherit unclear placement. Keep the concise happy path: an untyped source, a small operation, and an assignment should stay easy to write.

## Reuse without hiding the graph

`use` imports source and operation definitions, commands included, from another `.spit` file, but not its pipeline steps. That already supports a tool library such as `mrtrix.spit`: users import operations and assemble their own visible flow without repeating command templates. A stage groups steps and gives them defaults; it does not need to be a lexical module to justify its existence. Scoping every product to a stage would make normal cross-stage flow need exports or qualified names. Reconsider the visual mismatch in S1 without assuming all stages must become namespaces.

Stage-specific defaults are already useful and have had substantial design work; do not redesign them merely to make stages look like modules. The question is whether an operation declared inside a stage that is visible everywhere misleads authors, and whether that is best addressed by clearer placement, diagnostics, or real scoping. A module is a reuse and public-interface concept; a stage is an organisation and defaults concept.

A future composite operation or reusable subpipeline could expand a standard sequence into concrete DAG jobs. This is a proposal, not supported syntax. Prefer an explicit call that names its input products and receives named public outputs; importing a file alone should not silently add jobs. The library author declares the public interface, and SPIT infers dependencies between the internal steps. Inferring the public outputs from leaf nodes alone is ambiguous, because a QC by-product may be internal rather than part of the stable interface. Each invocation needs its own intermediate product identities and paths to avoid collisions. Keep every expanded job inspectable in the resolved DAG.

Customisation should be small and explicit: common behaviour works with defaults, and the library exposes only meaningful processing choices or optional inputs. Someone needing a substantially different sequence can import the constituent operations and compose it directly. Reusable checks can belong to primitive operations or to a composite's public output contract, while a calling pipeline adds analysis-specific checks. Applicable checks accumulate on the concrete artifact rather than silently overriding one another. Path placement, dataset exceptions, and site scheduling stay in their pipeline, recipe, or backend layers.

**R1, provenance and diagnostics.** Reuse saves repetition but can obscure which definition produced a job. Before building composites, decide how a DAG or inspection command identifies the imported file and version or content hash behind each expanded job, and how an error names both the caller's step and the failing internal step. Relative imports plus a pinned project revision may suffice at first; a package registry is not required. Test the model with a reusable sequence in a second domain as well as MRtrix, so the abstraction is not tailored to one toolchain.

## How users establish trust

Trust comes from a sequence of limited, inspectable claims, not from an undifferentiated "valid" label. The compiler can prove that the declared graph resolves under its source inventory and type and dimension rules, and can show the exact concrete jobs, paths, commands, dependencies, and checks it compiled. The editor should expose that compiler knowledge during authoring: for a future composite call, a hover or navigation view could show its public signature, constituent operations, outputs, checks, and where each definition came from. The runner then supplies the separate evidence of execution: command and check outcomes, missing outputs, and the state used for reruns. None of these layers alone proves the scientific correctness of an input file or analysis.

The VS Code extension already consumes compiler diagnostics and hovers for existing language elements; composite-call hovers and source tracing are proposed, not existing guarantees. Keep those explanations compiler-backed so the CLI and other editor integrations can expose the same facts. A useful acceptance test: can someone start at a short imported call, inspect every job it expands into, and follow an error back to the call and the exact internal definition without guessing?

## Compiler robustness as the language grows

The concern is whether new features will be built as reusable rules or as accumulating special cases. `docs/architecture.md` shows good structural foundations: a compiled `Pipeline`, a separate `SourceInventory`, resolution into a concrete DAG, and binding into a `BoundDag`. For example, `sidecars` syntax lowers to ordinary source products rather than needing a separate job model. This is evidence of a general approach, though the architecture document alone is not a line-by-line code audit.

The data-oriented performance work is a separate strength to keep. [Issue 37](https://github.com/eclnz/spit/issues/37) reports that PR 32 reduced a 4,000-step `spit check` case from 2,015 ms to 72 ms; that is the issue's reported benchmark, not a fresh measurement. When changing representations, check both semantic equivalence on examples and generated datasets and performance on the large cases; neither speed nor clean lowering alone establishes correctness.

Keep a small number of canonical intermediate representations and invariants. A new surface feature should enter at the phase that owns its meaning and then reuse downstream machinery wherever possible. A composite operation would ideally expand into ordinary invocations before standard resolution; an output check would become a bound check in the job contract; resource requests would become job metadata that backends interpret. Explicit parser cases are normal. A warning sign is one feature needing slightly different matching, path binding, error handling, and runner behaviour in many unrelated places without a shared representation.

For each proposed feature, document:

1. which phase owns it;
2. what existing internal form it lowers to, or why a new form is needed;
3. which invariants it must keep, such as acyclic dependencies, unique output paths, deterministic binding, and honest `Unknown` types;
4. how it interacts with selectors, imports, stages, paths, partial plans, and diagnostics.

Test those interactions and malformed input, not only the happy-path example that motivated the feature. This is a targeted audit; it does not require making every part of the compiler dynamically extensible.

## Targeted sidecar code trace

A narrow review of the usability branch, not a full compiler audit or a measured performance comparison. In `src/parser/flow.rs`, a `sidecars` block parses its members with the ordinary source declaration parser, copies the group's dimensions onto those products, and creates ordinary per-member path rules from the shared stem. The `SidecarGroup` model keeps group metadata for recipe paths and diagnostics; `InputRules::source_paths_for` resolves a recipe's group stem or default into per-source paths. Discovery then uses the standard source-path matcher. That is a good example of specialised syntax lowering into shared compiler data, keeping only the metadata needed for group-specific behaviour.

**T1.** The remaining group-specific diagnostic in `src/inputs/mod.rs` has a real weak point: `incomplete_groups` builds an identity by joining `dimension=value` fragments with commas, then checks removals by splitting that display string. A value containing `,` or `=` can be misread. Issue 37 already identifies this and proposes grouping by structured entity bindings and a member bitset. Fix that loss of structure and test values containing the delimiters; it does not call for replacing the data-oriented architecture.

*Done in `5ab3a16`.* Records are grouped by `GroupKey` with the members found, and removals match by symbol (`EntityBinding::within`). The delimiter case turned out to be unreachable: discovery already skips a value holding `,` or `=`, and this warning is given only for scanned inputs, so no test with such values can reach it. The change still removes the display string used as a key, halves `spit inputs` on a large sidecars dataset, and lists the warnings in value order (`shot=2` before `shot=10`).

**T2.** Check whether the group-level incomplete warning should appear only for scanned inputs: the `InputSource::Inventory` branch currently returns an empty incomplete-group list.

*Done in `8ef0e50`.* Decided: the warning describes the sources, not how they were found, so records in a recipe or a `.spitout` get it too. A removal the records already list under `removed:` counts, so a file an earlier scan excluded is not reported missing. `dag` and `artifacts` on a `.spitout` now print it. The editor's diagnosis never showed this warning, for scans or records, and still doesn't; making it a located diagnostic would belong with 4b.

General lesson: keep typed or interned identities through parsing, matching, resolution, and diagnostics; format them as text only when presenting a message. Some feature-specific handling is right when it owns real group semantics. The smell is duplicate interpretations of the same artifact identity in separate phases, particularly a presentation string reused as a key.

## Design principle for agents: lower cleanly, or draw a new boundary

Treat each new language feature as a question about representation, not as a list of places to add conditionals. First identify the feature's distinct meaning and the earliest compiler phase that can express it. Lower it into existing sources, path rules, operations, bindings, or DAG nodes when those forms keep its meaning and invariants. Keep only the extra metadata needed for a genuine feature-specific diagnostic or contract. Sidecars are largely an example of this; their incomplete-group diagnostic is legitimate, but turning a binding into a display string and parsing it back is not.

If the shared representation cannot express the feature faithfully, choose explicitly:

1. **Generalise the shared machinery** when the missing capability is useful to existing and future features too. Define the new invariant once and migrate existing paths to it, so the same resolver and diagnostics keep applying.
2. **Give the concept its own model and phase** when it has genuinely different semantics. Define a narrow, typed boundary where it produces or consumes the shared DAG; do not thread feature flags and parallel interpretations through unrelated phases. Runtime artifact checks, for example, could have their own check model while becoming explicit conditions on bound jobs.

Do not force a distinct concept into an unsuitable abstraction merely to claim reuse, and do not add a separate discovery, binding, and job path merely because the current model is inconvenient. In a feature review, show the lowering or boundary, the invariants kept, where source locations and errors come from, and which shared tests will catch regressions. If a change needs branches in many downstream phases, revisit the design before extending the language. Data-oriented storage and performance work can support either choice; this is about coherent semantics, not a prescribed class hierarchy.

## Future direction, deliberately deferred

- **Resources.** An operation could supply reusable CPU, memory, or time defaults; a `.spitin` recipe could override them for dataset outliers; a backend profile could supply cluster-specific account or partition settings. SPIT would bind the resolved portable values into the DAG. These are optional hints or requirements whose exact semantics need definition, not a reason to turn `.spit` into Slurm syntax.
- **Slurm.** A future adapter could submit DAG jobs as Slurm jobs with `afterok` dependencies. A per-job wrapper could run the relevant prechecks, the command, the output existence check, and the postchecks, and exit nonzero on failure; Slurm then records the result and holds dependent work. The adapter translates the shared job contract into submissions; it does not implement a second scheduler. Prove the shared success and rerun behaviour locally first. Job arrays can come later, since their dependency and partial-failure semantics are less direct.
- **One execution package.** If a second backend becomes real, spit-bash could become a more generally named runner with local and Slurm backends, sharing DAG interpretation, checks, and success rules, and keeping submission, cancellation, and recovery per backend. No repository restructure is needed for a hypothetical backend.
- **Static scope.** Known artifacts and dimensions are a strength for early validation. Workflows whose outputs are discovered only after a command runs stay a deliberate limitation, to reconsider only when a concrete use case warrants it.

## Questions to answer with concrete pipelines

1. Which artifact properties recur often enough to justify named reusable checks, and should those checks attach to ports, products, or both?
2. What exactly should happen when an optional sidecar or conditional output is absent?
3. Should a changed postcondition revalidate existing output files or recompute the producing job?
4. When a resource override belongs to a dataset rather than an operation, what is the simplest way to select its affected jobs?
5. Can users get from their own folder to a correct DAG without writing repetitive path rules or guessing where a declaration belongs?
6. In real pipelines, do stage-local definitions, minimum cardinality, recipe requirements, or command templates lead people to infer a different meaning than the compiler implements?
7. Can a reader trace every job created by a reusable component back to its call, internal step, and exact definition without reading hidden source files by guesswork?

The working principle: keep the flow short, keep the complete resolved contract in JSON, and add execution-specific detail only where a real workflow needs it.
