# Runnable DAGs

Once the plan has the expected jobs, save it:

<!-- spit-doc: fragment -->

```sh
spit dag dataset.spitin -o plan.spitdag
```

The `.spitdag` contains the concrete artifacts, paths, ordered jobs, command arguments, checks, and dependencies for one dataset. A runner such as [spit-bash](https://github.com/eclnz/spit-bash) consumes it; the runner does not parse the pipeline or recipe. Regenerate the DAG when the pipeline or inputs change.

The pipelines in [`examples/runnable/`](https://github.com/eclnz/spit/tree/dev/examples/runnable) call only `cp`, `sort`, `grep`, `tar` and `test`, so spit-bash can run them anywhere. Each has a recipe and a small input folder, and covers one feature: sorting and merging, operations carried out by steps, `verify` commands, checks, and folders as inputs and outputs.

Use `spit dag dataset.spitin --commands` to inspect the arguments first. The [DAG format manual](../spitdag.md) defines each field and its valid values. The [command line manual](../cli.md) lists the output and inspection flags.

Next: [Complete examples](../examples.md).
