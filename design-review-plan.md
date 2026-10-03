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
| 1 | Optional runtime checks in the job contract | claimed (`runtime-checks`) |
| 2 | Optional-file and output semantics, from one real workflow ([#35](https://github.com/eclnz/spit/issues/35)) | open |
| 3 | Lower the cost of binding existing data: judge `inputs --suggest` on irregular datasets | open |
| 4a | `dag --counts` | done (`370a18f`) |
| 4b | Diagnostic severity audit ([#45](https://github.com/eclnz/spit/issues/45)) | open |
| 5 | The two-file mental model | claimed (`two-files`) |
| S1 | Syntax: operations declared inside a stage | open |
| S2 | Syntax: `@ min(n)` beside the `many` port | decided, claimed (`many-min`) |
| S3 | Syntax: `require` and `drop` clause order ([#36](https://github.com/eclnz/spit/issues/36)) | open |
| S4 | Syntax: shell metacharacters in commands | open |
| S5 | Syntax: `path:` in both `.spit` and `.spitin` | claimed (`two-files`) |
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

### 2. Resolve optional-file and output semantics with a real workflow

[Issue 35](https://github.com/eclnz/spit/issues/35) leaves open optional sidecar members and outputs a command may or may not create, as well as the backend's missing-output contract. Work through one genuine case before adding a general optional type. If a sidecar is absent, the graph needs to say whether to skip the job, use a different operation, proceed without that argument, or fail. "Optional" syntax alone does not settle that behaviour.

### 3. Lower the cost of binding existing data

The project has improved path diagnostics and can scan a dataset without a recipe. [PR 52](https://github.com/eclnz/spit/pull/52), open when reviewed and since merged, adds `spit inputs --suggest` to generate source and path lines from existing files. This addresses a usability-study finding directly. Judge it by whether a user can inspect, correct, and trust suggestions for irregular datasets; keep suggested declarations editable and verify them with the actual matcher.

### 4. Make a resolved graph easy to sanity-check

A proposed `dag --counts` view would show total jobs and counts by operation. That would quickly reveal unintended expansion or an unexpectedly empty stage. It improves inspection rather than expressivity, so it follows the job-contract questions in priority.

*4a done in `370a18f`.* `dag --counts` prints one row per step (`cleaned = clean`, its stage, its job count), keeps steps with no jobs as `0`, then the total; it combines with `-o`. It counts the jobs planned: with `--partial`, the artifacts left out are not yet counted per step, which `IncompleteJob` would need a step id for.

Keep the severity of compiler feedback consistent with whether it blocks a valid graph. [Issue 45](https://github.com/eclnz/spit/issues/45) asks whether errors that do not fail `check` should instead be warnings, especially in the VS Code extension. Audit those cases at the compiler's diagnostic level so CLI and editor users see the same distinction; the editor should render severity rather than decide it.

### 5. Clarify the two-file mental model

Explain `.spit` in one sentence as the reusable graph and `.spitin` as a dataset binding and exception layer. Review each shared directive for clear default and override semantics. Keep the no-recipe `--root` path for simple cases rather than requiring a nearly empty recipe. Use real first-pipeline attempts to find where users cannot tell which file owns a declaration.

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
