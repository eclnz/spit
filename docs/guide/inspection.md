# Plans

SPIT checks a pipeline before running any work. A useful inspection loop is:

```sh
spit check pipeline.spit --path-rules
spit inputs dataset.spitin --unmatched
spit dag dataset.spitin --counts --commands
```

`check` reports syntax, type, dimension, command, and path errors. `--path-rules` shows which template each product takes. `inputs --unmatched` lists files that no source rule reads. `dag --counts` shows jobs per step, including steps with zero jobs; `--commands` shows the exact arguments a runner would receive. Once the plan is right, `spit dag dataset.spitin -o plan.spitdag` saves it.

To inspect branches and joins before choosing a dataset, run `spit dag pipeline.spit --tree`. Start with the stage overview, then follow the local diagrams. Follow each connected band across the page; successive bands reverse direction to keep product lines continuous. Lines connect operations within each stage; matching product names connect stage boundaries, and separate input arrows identify joins. Reusable operations have their own component diagrams, so you can follow the main pipeline before inspecting their suboperations. See the [command line reference](../cli.md#input-forms) for the output conventions.

## Missing jobs

1. Run `spit inputs dataset.spitin` and check how many source artifacts it found. A rule can match no files, or a path placeholder can capture an unexpected value.
2. Run `spit inputs dataset.spitin --unmatched` to find files outside all source rules. Add or correct source path rules for files the pipeline should read.
3. Run `spit artifacts dataset.spitin` to see complete artifacts, incomplete artifacts with reasons, coverage gaps, and unused sources.
4. Run `spit dag dataset.spitin --counts --commands` to inspect the plan that resolves, and `spit dag dataset.spitin --paths` to inspect artifact paths. A zero count can reveal a step whose driving source has no artifacts.

Identity values match as written: `store=s07` and `store=S07` are different. An unused source can therefore be a near miss for a failed join. A missing source declared `beside` another source produces a warning; the job that reads it can still fail. [Sidecar files](../language-reference.md#sidecar-files) and the [cohort walkthrough](../examples.md#cohort-discovery-exclusion-and-grouped-removal) show these cases.

## Partial plans

Plain `dag` stops at an incomplete job. `spit artifacts dataset.spitin` reports all incompleteness it can trace. If you need a plan for work whose inputs are complete, run:

```sh
spit dag dataset.spitin --partial -o plan.spitdag
```

The DAG records outputs it left out and their reasons. A `many` input uses its complete members, then checks any `@ min(n)` requirement. `--partial` is a choice for this planning run, not a recipe rule. Use conditional `exclude` for a repeatable data policy and `require` when missing coverage must fail the run. See [inputs and partial plans](../language-reference.md#inputs).

## Diagnostics

`spit check pipeline.spit --json --hovers` returns structured diagnostics, path hints, and word and symbol hovers. `--stdin` reads an unsaved buffer while keeping the file path for imports. The [command line reference](../cli.md) lists flags; the [VS Code extension](https://github.com/eclnz/spit-vscode) uses this interface.

Next: [Runnable DAGs](runnable.md).
