# Resolution

Resolution turns a `Pipeline` and a `SourceInventory` into jobs. `collect_definitions` first checks every declaration and step of the pipeline's text, with no data, and reports every error it finds. Only a pipeline with none compiles. The resolver expands checked steps over the inventory's artifacts. A command step makes jobs, and a directory view binds its required members for each context. The result is a `ResolvedDag` of logical artifacts and jobs, with no paths and no commands; [binding](binding.md) adds those.

```mermaid
flowchart LR
    pipeline["Pipeline"] --> check["collect_definitions"]
    check --> report["DefinitionCheck<br/>every error, poisoned names,<br/>steps that checked"]
    report --> diagnostics["check_pipeline<br/>and editor"]
    report --> compile["compile<br/>no error, or the first"]
    compile --> compiled["CompiledPipeline<br/>CompiledStep with StepShape per step"]
    compiled --> potential["resolve_potential_artifacts<br/>gates left to the runner"]
    inventory["SourceInventory"] --> potential
    held["held-back sources<br/>from CoverageGaps"] --> potential
    potential --> artifacts["ArtifactReport<br/>ResolvedDag with directory views,<br/>IncompleteJob"]
    artifacts --> resolve["resolve<br/>first unmatched gap, or the DAG"]
    artifacts --> bind["binding, then the runner,<br/>which applies live verdicts"]
```

## Data

- **`DefinitionCheck`** ([`src/compile/mod.rs`](https://github.com/eclnz/spit/blob/dev/src/compile/mod.rs)) is the check of a pipeline's declarations and steps. `collect_definitions` makes it, and `check_pipeline` runs it before the checks of rules, commands and paths (see [diagnostics](diagnostics.md)). It holds:
    - every error, each with the declaration or step it belongs to (`DefinitionSubject`);
    - the poisoned names: products and operations that failed, or that are made by a step that failed or reads from one;
    - the steps that checked. With errors, they are part of a pipeline, which the editor describes but nothing resolves.
- **`CompiledPipeline`** holds each step of a pipeline with no error, producers before consumers. Only `compile` makes one, from a `DefinitionCheck` with no errors; otherwise it returns the first error. `validate_pipeline` is `compile` without the result. Each `CompiledStep` holds:
    - its `Invocation` and `OperationDef`;
    - each output's product, with the type inferred for it;
    - its `Substitutions`;
    - its `StepShape`: the driving input, the dimensions each other input is matched on, the grouping dimensions for first and last selectors, and, for a step with `many` inputs, its `CollectionRule`: those ports, the dimensions that name a member, in the order the first one's `vary` writes them, and the minimum number of members ([`src/compile/steps.rs`](https://github.com/eclnz/spit/blob/dev/src/compile/steps.rs)).
- **`ResolvedDag`** ([`src/model/dag.rs`](https://github.com/eclnz/spit/blob/dev/src/model/dag.rs)) holds:
    - the jobs, each naming its step by `StepId`, with an ID local to this resolution, which changes when the inventory does;
    - the steps (`DagStep`), with each step's operation, stage and `CollectionRule` once, which binding writes as each of its jobs' `collection`;
    - the `Artifacts` table every job refers to by `ArtifactId`;
    - the directory views and each view's member file IDs;
    - the file of each source whose record gave one.
- **`ArtifactReport`** holds every source, the DAG of the jobs that can run, each `IncompleteJob` with its gaps, and each `CoverageGap`. A `Gap` is one of two kinds:
    - `Unmatched`: a missing or ambiguous input, or a collection below its minimum;
    - `Blocked`: an input that is itself incomplete, or a source a coverage gap holds back.

The entry points are in [`src/resolver/mod.rs`](https://github.com/eclnz/spit/blob/dev/src/resolver/mod.rs). Resolution plans potential work and applies no verdict: the compiler never reads a review's file. `resolve_potential_artifacts` keeps every job a verdict could admit, and binding carries each gate's attachments and member keys in the `.spitdag`. The runner reads the review files and screen records each time it runs, so a saved DAG serves every review round. Every command takes this route: `spit check`, `spit dag` and `spit artifacts` through `diagnose_checked_with_inventory` ([`src/diagnostics/mod.rs`](https://github.com/eclnz/spit/blob/dev/src/diagnostics/mod.rs)), and `dag --partial` again with `partial` set ([`src/cli/commands.rs`](https://github.com/eclnz/spit/blob/dev/src/cli/commands.rs)). `resolve(&pipeline, &inventory)` serves the library's callers and the tests: it gives the `ResolvedDag`, or fails at the first job that cannot be made.

A job that cannot be made is reported with its gaps rather than stopping resolution; a command fails on the first gap unless it is `spit artifacts` or `dag --partial`. Under `--partial`, a job whose `many` input has a member that cannot be made is left out like any other job with a gap, so an aggregate is never planned over fewer members than it names.

The saved-DAG boundary is checked by [`potential_dag_carries_review_scope_and_ignores_the_sheet_contents`](https://github.com/eclnz/spit/blob/dev/tests/reviews.rs) and [`a_review_plans_every_job_whatever_its_verdicts`](https://github.com/eclnz/spit/blob/dev/tests/reviews.rs) on the compiler's side, and by [`test_same_dag_resumes_and_changes_cached_paired_membership`](https://github.com/eclnz/spit/blob/dev/runner/tests/test_reviews.py) on the runner's. That `--partial` leaves out an aggregate with any member that cannot be made is checked by [`a_partial_plan_leaves_out_an_aggregate_with_any_member_that_cannot_be_made`](https://github.com/eclnz/spit/blob/dev/tests/cli.rs).

## Responsibilities

**Matching** ([`src/resolver/matching.rs`](https://github.com/eclnz/spit/blob/dev/src/resolver/matching.rs)):

- **Each step has a driving input, which gives it one job per context.**
    - In a preserve step, the driver is the single-artifact input with the most dimensions, provided every other input is matched on a subset of them. Port order never changes the choice. The outputs take the driver's dimensions.
    - In an aggregate step, the driver is its first `many` input, grouped by every dimension except those its `vary` names. Another `many` input must have the driver's dimensions and `vary`; each job takes its group, and an artifact whose varied values the other collection lacks is a `MissingInput` gap (`unpaired` in `src/resolver/matching.rs`). Matching records that gap's member, its values of the rule's member dimensions, so a deferred gap's `member` key comes from the same rule as the job's `collection`. Tests: [`test_a_missing_mate_over_two_dimensions_varied_in_reverse_blocks_only_its_member`](https://github.com/eclnz/spit/blob/dev/runner/tests/test_reviews.py), [`test_missing_paired_input_only_blocks_a_required_member`](https://github.com/eclnz/spit/blob/dev/runner/tests/test_reviews.py).
- **Every other input is matched to each job's context, and must leave exactly one artifact.**
- **Selectors narrow a binding.**
    - `where(dim=value)` keeps the matching artifacts, and takes the pinned dimension out of matching.
    - `first(dim)` and `last(dim)` group the artifacts that pass `where` by their remaining dimensions, keep one artifact in natural family order per group, and take `dim` out of matching. The selected artifact is subject to the port's ordinary availability and review checks.
    - `same(dim, ...)` matches on the listed dimensions alone.
    - `each(dim, ...)` broadcasts a single input over dimensions the driver lacks. The step runs for every combination of the driver's groups with the values that input has, and the outputs gain those dimensions. An input that broadcasts cannot drive.
- **A command step's driving artifacts create its contexts.** A command step whose driver has no artifact in a context has no job there.
- **A directory view matches over the union of its members' contexts.** Every member must match. A consumer's dependencies name the jobs that write the matched files.

**Potential work and gates.** `resolve_potential_artifacts` keeps potential readers and aggregate members. In a pipeline with reviews or screens, a missing input whose relevance depends on admission to the job or collection is planned as the job's `missing`, for the runner to judge once it knows the verdicts. `partial` turns that off, so such a job is left out instead.

**Artifact order.** Artifact families, jobs and collections follow the pipeline's dimension order, which lowering sets ([`src/order.rs`](https://github.com/eclnz/spit/blob/dev/src/order.rs)). Runs of digits compare as numbers, so `wave2` comes before `wave10`.

**Types** ([`src/compile/types.rs`](https://github.com/eclnz/spit/blob/dev/src/compile/types.rs)). Type checking is separate from matching, and optional.

- An omitted type is `TypeExpr::Unknown`. Names, dimensions and cardinality drive resolution even when every type is unknown.
- `infer_types` checks each step once, in dependency order, so a known contradiction fails with no inventory at all.
    - Each step unifies its ports with its products under fresh variables.
    - Each output takes its port's type with the variables substituted, and unresolved variables erased to `Unknown`, so no variable leaks into another step.
    - Every job of the step takes those types.
- `Unknown` leaves compatibility undecided. It is never a conflict.
- `check_calls` holds each call of an operation carried out by steps to the operation's own signature, under one set of variables per call.
- Under `types strict` (`TypeChecking::Strict`), `check_strict` ([`src/compile/strict.rs`](https://github.com/eclnz/spit/blob/dev/src/compile/strict.rs)) runs once inference is done. It reports each source, each port of an operation a step or call uses, and each output of a checked step whose type has an `Unknown` part. Inference erases a variable no input fixes to `Unknown`, so an open variable is reported at the product, where a written type fixes it. A step that reads a product or runs an operation already reported is not reported again. `Pipeline::types` comes from the file given to SPIT, never from an import.
- The type engine has structural unification and type variables, with no subtypes: bare `MRI` is not a supertype of `MRI<T1w,Anatomical>`. `Pipeline::type_decls` holds the parameter names of each `type Name<Param, ...>`, and `check_declared` ([`src/compile/declared.rs`](https://github.com/eclnz/spit/blob/dev/src/compile/declared.rs)) holds every source, port and typed product to its declaration's arity, nested types included, before inference runs. A name with no declaration is checked only under `types strict`, which asks for one when it is written with arguments. A bad type poisons its source or operation. Arguments stay positional, so unification does not read the names. Named arguments are not built.

**Not done here:**

- settling the dataset, which the [input stage](inputs.md) does before resolution;
- paths and commands, which [binding](binding.md) adds.

## Invariants

- **Jobs come in dependency order, numbered from 1.** Every input's producer is expanded before its consumer is matched, and a job's ID is its place in that order. Test: [`tracks_dependencies_through_full_pipeline`](https://github.com/eclnz/spit/blob/dev/tests/resolver.rs).
- **An incomplete job's outputs stay candidates for later steps.** A consumer of one is reported as blocked, not left out without a word. Test: [`keeps_complete_jobs_and_blocks_consumers_of_incomplete_ones`](https://github.com/eclnz/spit/blob/dev/tests/artifacts.rs).
- **A blocked gap always follows the gap that blocks it.** So the first unmatched gap explains every failure, and `resolve` returns it (`first_failure`). Test: [`resolve_fails_with_the_first_gap_the_report_finds`](https://github.com/eclnz/spit/blob/dev/tests/artifacts.rs).
- **A coverage gap holds back its sources.** A job that needs one is blocked, as it would be by an incomplete producer. Test: [`a_coverage_gap_holds_back_its_sources_and_blocks_their_consumers`](https://github.com/eclnz/spit/blob/dev/tests/artifacts.rs).
- **Each artifact is in the `Artifacts` table once, and ids count up from 0 in the order they are added.** `Artifacts::add` gives the id of an artifact already there, and `Artifacts::product` keeps the first type a product is given. Tests: [`adding_an_artifact_twice_gives_its_id_and_keeps_one_row`](https://github.com/eclnz/spit/blob/dev/src/model/artifacts.rs), [`a_product_is_numbered_once_and_keeps_the_first_type_it_is_given`](https://github.com/eclnz/spit/blob/dev/src/model/artifacts.rs), [`find_gives_what_add_added_and_nothing_else`](https://github.com/eclnz/spit/blob/dev/src/model/artifacts.rs), [`ids_count_up_from_zero_in_the_order_artifacts_are_added`](https://github.com/eclnz/spit/blob/dev/src/model/artifacts.rs).
- **Types are checked before any job is made.** Test: [`checks_types_before_concrete_expansion`](https://github.com/eclnz/spit/blob/dev/tests/resolver.rs).
- **Each group of steps in a loop is one error, and a pipeline with one never resolves.** The steps that read from a loop are not checked, and the rest of the pipeline still is, in dependency order. Tests: [`every_independent_cycle_is_reported_once_and_quiets_what_reads_it`](https://github.com/eclnz/spit/blob/dev/tests/diagnostics.rs), [`a_cycle_is_reported_once_at_the_step_it_was_found_at`](https://github.com/eclnz/spit/blob/dev/tests/diagnostics.rs).
- **Under `types strict`, a pipeline whose types inference completes passes, and a missing type is reported once, where it is missing.** Tests: [`a_type_declared_once_and_inferred_through_generic_steps_passes`](https://github.com/eclnz/spit/blob/dev/tests/strict_types.rs), [`an_untyped_source_is_reported_at_its_declaration_and_quiets_what_reads_it`](https://github.com/eclnz/spit/blob/dev/tests/strict_types.rs).
- **A declared type is written with as many arguments as it has parameters, wherever it is written.** Tests: [`a_source_with_too_few_or_too_many_arguments_is_reported_where_it_is_written`](https://github.com/eclnz/spit/blob/dev/tests/declared_types.rs), [`a_nested_type_is_checked_in_turn`](https://github.com/eclnz/spit/blob/dev/tests/declared_types.rs).
- **A failure is reported once, not again by what reads it.** A step that uses a poisoned name is skipped, and poisons its own outputs. Tests: [`uses_of_a_failed_declaration_or_step_are_not_reported_again`](https://github.com/eclnz/spit/blob/dev/tests/diagnostics.rs), [`a_product_two_steps_make_quiets_every_step_that_reads_it`](https://github.com/eclnz/spit/blob/dev/tests/diagnostics.rs), [`every_independent_cycle_is_reported_once_and_quiets_what_reads_it`](https://github.com/eclnz/spit/blob/dev/tests/diagnostics.rs).

## Failure

- A pipeline error, such as an unknown operation, a type conflict or a cycle, fails `compile` before any job is made. Each is a `ResolveError` ([`src/model/error.rs`](https://github.com/eclnz/spit/blob/dev/src/model/error.rs)).
- `collect_definitions` reports every error that does not follow from another:
    - each declaration's check gives its first fault;
    - a step's shape is checked only once its types hold, since a product of the wrong type at a port is most often an argument in the wrong place, which would make the shape wrong too;
    - a product two steps make is reported as a duplicate declaration, not also as having more than one producing invocation;
    - a product whose path rule names a dimension no product has is not also checked for the dimensions its rule omits, since the unknown name is most often a misspelling of one it has;
- The command line diagnoses a file before `inputs`, `dag` or `artifacts` uses it, so each of those prints every error, not only the first ([diagnostics](diagnostics.md)).
- An input the inventory cannot supply makes a job incomplete. `resolve` fails with the first such gap, and the report keeps them all.
