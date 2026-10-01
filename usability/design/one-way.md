# Design: one way to write each thing

The [second study](../ROUND2.md) produced 18 correct plans, but no two participants wrote the same pipeline. They differed on these points:

- whether to use types;
- which input drives a sweep;
- where a dimension order is set;
- whether to write `@ vary`;
- whether a cohort rule lives in the pipeline or the recipe.

Every variant passed, so the language does not yet decide these things. The language is young enough to settle them now, before more examples and users depend on the alternatives. This plan settles them. Each decision below either gives the language one form, or sets a principle for a later round to test.

## The decisions

| # | Question | Decision | When |
| --- | --- | --- | --- |
| 1 | Which dimension an aggregate collapses: the operation's `@ drop` or the call's `@ vary`? | The call, with `@ vary`. Operations lose `@ drop`. | Now |
| 2 | Where a product's dimension order comes from | One order for the whole pipeline. | Now |
| 3 | What belongs in the pipeline and what in the recipe | Keep the current split; test a revision-change task before adding recipe selection. | Decided after round 3 |
| 4 | What types are for | Keep optional types and positional checks; teach distinct role types that catch a wrong connection. | Decided after round 3 |
| 5 | Syntax doubles | Keep one form of each. | Now |

## 1. The call says which dimensions are collected

**Status:** done in step 19.

**Today.**
- An aggregate is written on the operation (`-> Report @ drop(day)`) and on the call (`report = rollup(digest @ vary(day))`), and the two must agree.
- Since step 13, the call may leave out `@ vary` when the operation has `@ drop`.
- In round 2, about two thirds of participants wrote both clauses and a third wrote only `@ drop`. One called the repetition a burden without knowing `@ vary` was optional.

**Problems.**
- **Reuse.** An operation that names its dimension serves only that dimension. A generic mean cannot average runs in one step and sessions in the next. `s2-cohort-b` wrote `average` and `combine` as separate operations for this reason.
- **Two places to read.** The change of grain is a fact about the data flow, but a reader of the flow cannot see it at the call when `@ vary` is omitted.
- **An overloaded word.** `drop` also names the recipe rule that removes groups (`drop [sub] where …`).

**Change.**
- **Every `many` input names its dimensions at the call:** `avg = mean(coreg @ vary(run))`. A call whose `many` input has no `@ vary` is an error that names the port.
- **The operation only declares `many`:** `operation mean(images: many Image) -> Image`.
- **`@ drop` on an operation is an error** that shows the call to write: "operations no longer name the dimensions they collect; write `@ vary(run)` on the call, as in `avg = mean(coreg @ vary(run))`".
- **`@ min(n)` stays on the operation.** "A fit needs at least two points" is a fact about the tool, not about one call.
- **`drop` keeps one meaning,** the recipe rule.

This reverses [F10's inference](language.md#the-calls--vary-follows-from-the-operations--drop): mark that section superseded.

**Where.**
- `OperationDef::aggregated_dimensions` (`src/model.rs`) is removed, and so is `effective_binding` (`src/shape.rs`).
- The contract checks that compare `@ drop` with `@ vary` (`src/compile/steps.rs`, near lines 91 and 135) are removed.
- The `@ drop` clause is removed from the operation parser (`src/parser/operation.rs`), which keeps a targeted error for it.
- Lowering (`inferred_dimensions` in `src/lower.rs`) already reads the call's binding, so output dimensions are unchanged.

**Tests.**
- A `many` call without `@ vary` fails, naming the port.
- `@ drop` on an operation fails with the suggested call.
- One `many` operation called twice, collecting different dimensions.
- `@ min` still applies.

**Rewrites.** Every `.spit` under `examples/`, every `tests/` pipeline, and the harness keys: 100 `@ drop` clauses in 46 files. The study's result folders are records and stay as they are.

## 2. One dimension order for the pipeline

**Status:** done in step 20.

**Today.**
- A derived product takes its driver's dimension order, then any `@ each` dimensions appended.
- The only way to change it is to annotate a product: `summary : Summary [model, config] = …`.
- Order decides the `many` collection order (so the order of command arguments), `{entities}`, and display.
- Both `s3-one-board` participants needed the override, and they put it on different products: `weights` and `summary`. The ragged-sweep example has to explain why `trained` comes out `[config, seed, model]`.

**Change.** A pipeline has one dimension order, and every product, source or derived, lists its dimensions in that order.

- **Where the order comes from.** Each source declaration states the relative order of its own dimensions; `source bold [sub, ses, run]` says `sub` before `ses` before `run`. Most pipelines need nothing more.
  - When some product holds two dimensions that no source orders, the pipeline must declare the order once. For example, `trained` holds `model` and `config`, which never share a source:

    ```text
    dimensions [model, config, seed]
    ```

    Without that line, `check` fails: "`trained` has dimensions `model` and `config`, which no source orders; add `dimensions [model, config, seed]`".
  - Order is never taken from the order lines happen to appear in.
- **Agreement.**
  - A `dimensions` line lists every dimension in the pipeline.
  - Each source must list its dimensions in that order.
  - Two sources that order a pair differently are an error naming both.
  - An imported file's sources must agree with the importing pipeline's order.
- **Placement.** An `@ each` dimension takes its place in the order instead of being appended.
- **Annotations become checks.** `summary : Summary [model, config] = …` must name the inferred set in the pipeline's order; it no longer reorders anything.
- **What it decides.** `many` collection order, `{entities}`, the identities written in `dag`, `artifacts`, the `.spitout` and the `.spitdag`.

**Why derived, with a line only where needed.** The single-order rule removes the per-product override, so there is one place to look. Deriving the order from sources keeps it free for the common case: BIDS, logs, stations, survey waves. Requiring the line only when nothing else decides avoids a silent accident. Every round 2 sweep participant happened to declare `model` first; under a first-appearance rule, a pipeline that declared `seed` first would quietly have collected config-first.

**Where.**
- **Computing the order.** A new pass after lowering computes the pipeline order from the sources and the `dimensions` line, as a topological sort that reports conflicts and unordered pairs. It sits beside the declaration checks in `src/compile/`.
- **Ordering products.** `step_context` (`src/shape.rs`) sorts a step's context by the pipeline order instead of appending broadcast dimensions. `inferred_dimensions` (`src/lower.rs`) does the same for derived products.
- **Parsing.** `dimensions [...]` is a new top-level pipeline statement (`src/parser/`). It is rejected in a recipe.
- **Records.** Check that `.spitout` records are read regardless of the order their entities are written in. Write them in pipeline order.

**Tests.**
- An unordered pair is an error naming the product and the two dimensions.
- Conflicting sources are an error naming both.
- An `@ each` dimension lands in pipeline order.
- The ragged sweep collects model-first with a `dimensions` line and no annotation.
- An annotation in a different order is an error.

**Rewrites.**
- The ragged-sweep walkthrough and example (`docs/examples.md`, `examples/patterns/ragged_sweep/`).
- The language-reference text on `each` placement and on explicit output dimensions.
- [Where an `each` dimension goes](language.md#where-an-each-dimension-goes) is superseded.
- The `s3-sweep` and `s3-one-board` keys gain a `dimensions` line. Their jobs and argument orders stay the same.

## 3. The pipeline says how to compute; the recipe says which data (after round 3)

This is a principle to test, not a change to make yet. Three features blur it today:

- **`where(revision=3)`.** The approved calibration revision is a fact about one dataset, but the selector puts it in the pipeline. Every round 2 participant put it there, because no recipe rule says it. A candidate is a recipe rule such as `select calibration[revision=3]`, with `where` kept for structural pins.
- **`@ min(2)` beside `drop … count<2`.** Keep both, with a sharper rule:
  - `@ min` is what the tool needs;
  - `drop` is cohort policy.

  The guide should say so with `s2-cohort-b`'s duplication as the example.
- **Source path rules** may be in either file. The reference already says which to choose. No change.

**Round 3 decision:** keep the current boundary. Both vague-sensor participants put `where(revision=3)` in the pipeline and completed the task, but neither had to change the approved revision for a new dataset. That observation does not justify a new recipe `select` rule yet. Test a revision-change request before designing one. Keep `@ min` as an operation requirement and `drop` as a dataset cohort rule; the cohort follow-ups used recipe exclusion without changing aggregation syntax.

## 4. Types catch wrong connections (after round 3)

Round 2 used four styles, and none stopped a wrong connection:

- **No types.**
- **One type for everything.** In `s2-cohort-a`, `coregister(mc: Image, brain: Image)` would accept its arguments swapped.
- **One type per product** (`Digest`, `Fleet`, `Clean`). This only restates the product name.
- **Product names written where types go.** Both `s4-vague` participants started this way.

**Proposed idiom:** type the ports that a swap would break. For example, give `coreg(bold: Bold, ref: T1)` distinct types rather than one per product.

**Checks for round 3:**
- whether participants who see that idiom in the examples follow it;
- whether one-type-per-product disappears when the examples stop using it.

**Round 3 decision:** keep optional types and positional type checks. One vague-sensor pipeline used distinct role types, while the other was untyped. One cohort pipeline used `Image` for both BOLD and T1w, which would not reject a swap, and the other left input ports untyped. The successful plans do not establish protection from wrong connections. Add an example with distinct input types and a failed swapped call, then test whether participants adopt that idiom. Do not remove type checking on the strength of these passing plans.

## 5. One form for each syntax double

**Status:** done in step 21, with one change: brace escapes stay as Bash quoting gives them.

| Today | Keep | Remove |
| --- | --- | --- |
| Grouped `products:`, `operations:`, `pipeline:`, `commands:` sections, and a recipe's `constraints:` | The flow form | The sectioned form (`src/parser/sectioned.rs`), used by 8 examples and some parser tests. No round 1 or 2 participant wrote it |
| `source x`, `source x []` | `source x` | `[]`, with an error that shows the bare form |
| `name`, `name: Type`, `name: many`, `many name`, a lone `many`, a type alone (`(Image)`) | `name`, `name: Type`, `name: many`, `name: many Type` | `many name`, a lone `many`, and nameless ports |
| `{input}`, `{input1}`, `{inputs}` for nameless ports, and `{inputs}` as an alias for any lone `many` port | Each port's own name | The positional placeholders and the alias |
| `{{` and `\{` for a literal brace | `{{` and `}}` in the guide | Nothing: a command follows Bash's quoting, so `\{` and `'{'` are literal as a consequence of that rule, not a second syntax. Rejecting `\{` would add an exception; the reference shows `{{` only |
| `.spitout` records ending in `: path` | Path rules | The legacy record form |
| A product named like its operation (`digest = digest(log)`) | Allowed | No removal; `check` warns, and the reference's `coreg = coreg(mc, brain)` example changes |

Each removed form gets an error that shows the kept form, so an agent trained on the old guide recovers in one call.
